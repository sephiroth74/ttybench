//! The payload's workloads, run on a PTY of the tests' own: it tells its
//! grid, prints its lines and its file, reads its keys, and tells each
//! through the FIFO.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// A PTY of 100 columns and 30 rows: its master and its slave.
fn pty() -> (OwnedFd, OwnedFd) {
    let (mut master, mut slave) = (0, 0);
    let mut size = libc::winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: the two descriptors and the size are writable; `openpty`
    // takes no name nor termios of null.
    let status = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut size,
        )
    };
    assert_eq!(status, 0);
    // SAFETY: `openpty` gave two descriptors that nothing else owns.
    unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) }
}

/// The payload of `args` on a PTY, the FIFO of its events, and what it
/// prints, as a thread drains the master.
struct Run {
    child: Child,
    events: BufReader<File>,
    master: File,
    printed: Arc<Mutex<Vec<u8>>>,
    dir: std::path::PathBuf,
}

fn run(name: &str, args: &[&str]) -> Run {
    let dir = std::env::temp_dir().join(format!("ttybench-payload-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("events");
    let path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: a path of no NUL, of the mode of its owner alone.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let (master, slave) = pty();
    let slave = File::from(slave);
    let child = Command::new(env!("CARGO_BIN_EXE_ttybench-payload"))
        .arg("--fifo")
        .arg(&fifo)
        .arg("--exit")
        .args(args)
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        .spawn()
        .unwrap();
    let events = BufReader::new(File::open(&fifo).unwrap());
    let master = File::from(master);
    let printed = Arc::new(Mutex::new(Vec::new()));
    let drained = Arc::clone(&printed);
    let mut reader = master.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                break;
            }
            drained.lock().unwrap().extend_from_slice(&buffer[..read]);
        }
    });
    Run {
        child,
        events,
        master,
        printed,
        dir,
    }
}

impl Run {
    /// The next event.
    fn event(&mut self) -> Value {
        let mut line = String::new();
        self.events.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("an event: {line:?}"))
    }

    /// What it printed so far.
    fn printed(&self) -> String {
        String::from_utf8_lossy(&self.printed.lock().unwrap()).into_owned()
    }

    fn end(mut self) {
        assert!(self.child.wait().unwrap().success());
        let _ = self.master.into_raw_fd();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn it_tells_its_grid_and_prints_its_lines() {
    let mut run = run("lines", &["lines", "--count", "50", "--width", "10"]);
    let ready = run.event();
    assert_eq!(ready["event"], "ready");
    assert_eq!(
        (ready["cols"].as_u64(), ready["rows"].as_u64()),
        (Some(100), Some(30))
    );
    assert_eq!(ready["pid"].as_u64(), Some(u64::from(run.child.id())));
    let done = run.event();
    assert_eq!(
        (done["event"].as_str(), done["count"].as_u64()),
        (Some("done"), Some(50))
    );
    assert!(done["time_ns"].as_u64() >= done["started_ns"].as_u64());
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        run.printed().contains("      49 xyzabcdefg")
            && run.printed().contains("       0 abcdefghij"),
        "{}",
        run.printed()
    );
    run.end();
}

#[test]
fn it_prints_a_file_to_its_last_byte() {
    let file = std::env::temp_dir().join(format!("ttybench-payload-file-{}", std::process::id()));
    std::fs::write(&file, "x".repeat(300 * 1024)).unwrap();
    let mut run = run("cat", &["cat", "--file", file.to_str().unwrap()]);
    assert_eq!(run.event()["event"], "ready");
    let done = run.event();
    assert_eq!(done["bytes"].as_u64(), Some(300 * 1024));
    run.end();
    let _ = std::fs::remove_file(&file);
}

/// Waits for `run` to have printed `text`, for a second at most.
fn printed_within(run: &Run, text: &str) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while std::time::Instant::now() < deadline {
        if run.printed().contains(text) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[test]
fn it_reads_its_keys_each_told_and_the_line_discipline_echoes_them() {
    let mut run = run("keys", &["keys", "--count", "2"]);
    assert_eq!(run.event()["event"], "ready");
    run.master.write_all(b"xy").unwrap();
    let first = run.event();
    let second = run.event();
    assert_eq!(
        (first["event"].as_str(), first["byte"].as_u64()),
        (Some("key"), Some(u64::from(b'x')))
    );
    assert_eq!(second["byte"].as_u64(), Some(u64::from(b'y')));
    let done = run.event();
    assert_eq!(
        (done["event"].as_str(), done["count"].as_u64()),
        (Some("done"), Some(2))
    );
    assert!(printed_within(&run, "xy"), "{:?}", run.printed());
    run.end();
}

#[test]
fn of_its_own_echo_it_echoes_its_keys_in_raw_mode() {
    let mut run = run("keys-raw", &["keys", "--count", "2", "--echo", "payload"]);
    assert_eq!(run.event()["event"], "ready");
    run.master.write_all(b"ab").unwrap();
    for _ in 0..2 {
        assert_eq!(run.event()["event"], "key");
    }
    assert_eq!(run.event()["event"], "done");
    assert!(printed_within(&run, "ab"), "{:?}", run.printed());
    run.end();
}

#[test]
fn of_a_line_its_keys_go_on_the_first_row_cleared_after_each_line() {
    let mut run = run("keys-line", &["keys", "--count", "5", "--line", "2"]);
    assert_eq!(run.event()["event"], "ready");
    // The screen cleared, the cursor at its top left.
    assert!(printed_within(&run, "\x1b[H\x1b[2J"), "{:?}", run.printed());
    for key in b"abcde" {
        run.master.write_all(&[*key]).unwrap();
        assert_eq!(run.event()["byte"].as_u64(), Some(u64::from(*key)));
    }
    assert_eq!(run.event()["count"].as_u64(), Some(5));
    assert!(
        printed_within(&run, "ab\r\x1b[Kcd\r\x1b[Ke"),
        "{:?}",
        run.printed()
    );
    run.end();
}

#[test]
fn an_echo_of_no_one_is_refused() {
    let status = Command::new(env!("CARGO_BIN_EXE_ttybench-payload"))
        .args(["keys", "--echo", "nobody"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

#[test]
fn a_workload_that_is_none_is_refused() {
    let status = Command::new(env!("CARGO_BIN_EXE_ttybench-payload"))
        .args(["nothing"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}
