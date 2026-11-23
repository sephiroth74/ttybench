//! The payload that a tool runs in a terminal, `ttybench-payload`: the
//! script that a terminal's launch runs, which runs the payload of a
//! workload and the FIFO that it tells its events through, and the events,
//! as the tool reads them.

use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::Value;

use crate::terminal;
use crate::tools;

/// A payload of a workload, ready to run in a terminal: its folder, its
/// script, its FIFO, and its events as a thread reads them.
#[derive(Debug)]
pub struct Payload {
    dir: PathBuf,
    command: PathBuf,
    fifo: PathBuf,
    events: Receiver<Value>,
}

/// The payload's binary: `ttybench-payload` beside the tool or on the
/// `PATH`, as the dispatcher finds the tools.
pub fn binary() -> Result<PathBuf, String> {
    let dirs = tools::search_dirs(
        std::env::current_exe().ok().as_deref(),
        std::env::var_os("PATH").as_deref(),
    );
    tools::find("payload", &dirs).ok_or_else(|| {
        "there is no ttybench-payload beside the tool or on the PATH, which the terminals run"
            .to_owned()
    })
}

impl Payload {
    /// The payload of `binary` running `workload`, as `["rest"]`, in a
    /// folder of its own.
    pub fn new(binary: &Path, workload: &[&str]) -> io::Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "ttybench-{}-{}",
            std::process::id(),
            crate::clock::now_ns()
        ));
        std::fs::create_dir_all(&dir)?;
        let fifo = dir.join("events");
        let path =
            std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).map_err(io::Error::other)?;
        // SAFETY: a path of no NUL, of the mode of its owner alone.
        if unsafe { libc::mkfifo(path.as_ptr(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        let mut line = format!("exec {} --fifo {}", quote(binary), quote(&fifo));
        for arg in workload {
            line.push(' ');
            line.push_str(&quote(Path::new(arg)));
        }
        let command = terminal::script(&dir, "run", &line)?;
        let (sender, events) = mpsc::channel();
        let reading = fifo.clone();
        std::thread::spawn(move || {
            let Ok(file) = std::fs::File::open(&reading) else {
                return;
            };
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else {
                    break;
                };
                if let Ok(event) = serde_json::from_str(&line)
                    && sender.send(event).is_err()
                {
                    break;
                }
            }
        });
        Ok(Self {
            dir,
            command,
            fifo,
            events,
        })
    }

    /// The script that a terminal's launch runs.
    pub fn command(&self) -> &Path {
        &self.command
    }

    /// The next event, within `timeout`.
    pub fn next(&self, timeout: Duration) -> Option<Value> {
        self.events.recv_timeout(timeout).ok()
    }

    /// The next event of the kind `event`, as `ready`, within `timeout`,
    /// those of other kinds passed over.
    pub fn wait(&self, event: &str, timeout: Duration) -> Option<Value> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.checked_duration_since(std::time::Instant::now())?;
            let next = self.next(left)?;
            if next["event"] == event {
                return Some(next);
            }
        }
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        // A reader waiting for a writer that never came goes, of a writer
        // that opens and closes.
        if let Ok(path) = std::ffi::CString::new(self.fifo.to_string_lossy().as_bytes()) {
            // SAFETY: a path of no NUL; the descriptor is closed at once.
            unsafe {
                let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK);
                if fd >= 0 {
                    libc::close(fd);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
