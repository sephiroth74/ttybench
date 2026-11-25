//! `ttybench-payload`: the program that ttybench runs inside every
//! terminal, so that the terminal hosts it alone. It tells the tool that
//! it is ready through a FIFO, of the grid that it got, its process and
//! the time of the clock that the tools share; runs its workload; tells
//! its end; then rests until the terminal quits.
//!
//! ```text
//! ttybench-payload [--fifo <path>] [--exit] <workload> [options]
//!   rest  [--seconds <n>]                 rest, of no output
//!   lines [--count <n>] [--width <n>]     print lines of a width, numbered
//!   cat   --file <path>                   print a file, timed to its last byte
//!   keys  [--count <n>] [--echo tty|payload] [--line <n>]
//!                                         read keys, each told
//! ```
//!
//! `keys` reads `count` keys, 1 by default, 0 for as many as come. The
//! terminal's line discipline echoes each, as it does to `cat` at a
//! prompt, or, of `--echo payload`, the payload does, in raw mode. Of
//! `--line`, the keys go on the first row of a cleared screen, as at a
//! terminal's start, cleared after every `line` keys, so that they never
//! wrap nor scroll.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Value, json};
use ttybench::clock::now_ns;

/// What the payload does once ready.
enum Workload {
    Rest { seconds: Option<u64> },
    Lines { count: u64, width: usize },
    Cat { file: PathBuf },
    Keys { count: u64, echo: Echo, line: u64 },
}

/// Who echoes the keys that `keys` reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Echo {
    /// The terminal's line discipline, of the kernel.
    Tty,
    /// The payload, in raw mode.
    Payload,
}

/// Its arguments.
struct Options {
    fifo: Option<PathBuf>,
    exit: bool,
    workload: Workload,
}

fn main() -> ExitCode {
    let options = match parse(std::env::args().skip(1).collect()) {
        Ok(options) => options,
        Err(why) => {
            eprintln!("ttybench-payload: {why}");
            return ExitCode::from(2);
        }
    };
    let mut events = match Events::open(options.fifo.as_deref()) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("ttybench-payload: the FIFO does not open: {error}");
            return ExitCode::from(1);
        }
    };
    let (cols, rows) = grid();
    events.tell(&json!({
        "event": "ready",
        "pid": std::process::id(),
        "cols": cols,
        "rows": rows,
        "time_ns": now_ns(),
    }));
    let done = match run(&options.workload, &mut events) {
        Ok(done) => done,
        Err(error) => {
            events.tell(
                &json!({ "event": "failed", "error": error.to_string(), "time_ns": now_ns() }),
            );
            return ExitCode::from(1);
        }
    };
    events.tell(&done);
    if !options.exit {
        // Rest until the terminal quits, which ends the process.
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
    ExitCode::SUCCESS
}

/// The options of `args`, or why they are none.
fn parse(args: Vec<String>) -> Result<Options, String> {
    let mut fifo = None;
    let mut exit = false;
    let mut rest = args.into_iter().peekable();
    while let Some(arg) = rest.peek() {
        match arg.as_str() {
            "--fifo" => {
                rest.next();
                fifo = Some(PathBuf::from(rest.next().ok_or("--fifo takes a path")?));
            }
            "--exit" => {
                rest.next();
                exit = true;
            }
            _ => break,
        }
    }
    let name = rest.next().ok_or("a workload: rest, lines, cat or keys")?;
    let mut values = std::collections::BTreeMap::new();
    while let Some(flag) = rest.next() {
        let key = flag
            .strip_prefix("--")
            .ok_or_else(|| format!("`{flag}` is no option"))?
            .to_owned();
        let value = rest
            .next()
            .ok_or_else(|| format!("--{key} takes a value"))?;
        values.insert(key, value);
    }
    let number = |key: &str, default: Option<u64>| -> Result<Option<u64>, String> {
        values.get(key).map_or(Ok(default), |value| {
            value
                .parse()
                .map(Some)
                .map_err(|_| format!("--{key} takes a whole number, not `{value}`"))
        })
    };
    let workload = match name.as_str() {
        "rest" => Workload::Rest {
            seconds: number("seconds", None)?,
        },
        "lines" => Workload::Lines {
            count: number("count", Some(1000))?.unwrap_or(1000),
            width: usize::try_from(number("width", Some(80))?.unwrap_or(80))
                .map_err(|e| e.to_string())?,
        },
        "cat" => Workload::Cat {
            file: values
                .get("file")
                .map(PathBuf::from)
                .ok_or("cat takes --file")?,
        },
        "keys" => Workload::Keys {
            count: number("count", Some(1))?.unwrap_or(1),
            echo: match values.get("echo").map(String::as_str) {
                None | Some("tty") => Echo::Tty,
                Some("payload") => Echo::Payload,
                Some(other) => {
                    return Err(format!("--echo takes tty or payload, not `{other}`"));
                }
            },
            line: number("line", Some(0))?.unwrap_or(0),
        },
        other => {
            return Err(format!(
                "`{other}` is no workload: rest, lines, cat or keys"
            ));
        }
    };
    Ok(Options {
        fifo,
        exit,
        workload,
    })
}

/// Runs `workload`, telling its keys through `events`: its end's record.
fn run(workload: &Workload, events: &mut Events) -> io::Result<Value> {
    let mut out = io::stdout().lock();
    match workload {
        Workload::Rest { seconds } => {
            if let Some(seconds) = seconds {
                std::thread::sleep(std::time::Duration::from_secs(*seconds));
            }
            Ok(json!({ "event": "done", "workload": "rest", "time_ns": now_ns() }))
        }
        Workload::Lines { count, width } => {
            // Each line's letters start a letter on from the last's, so that
            // no two lines near each other look alike.
            let letters: String = (0..*width + 26)
                .map(|at| char::from(b'a' + u8::try_from(at % 26).unwrap_or(0)))
                .collect();
            let started = now_ns();
            for at in 0..*count {
                let from = usize::try_from(at % 26).unwrap_or(0);
                writeln!(out, "{at:>8} {}", &letters[from..from + *width])?;
            }
            out.flush()?;
            let ended = now_ns();
            Ok(json!({
                "event": "done", "workload": "lines", "count": count,
                "started_ns": started, "time_ns": ended,
            }))
        }
        Workload::Cat { file } => {
            let mut source = File::open(file)?;
            let mut buffer = vec![0_u8; 64 * 1024];
            let mut bytes = 0_u64;
            let started = now_ns();
            loop {
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                out.write_all(&buffer[..read])?;
                bytes += read as u64;
            }
            out.flush()?;
            let ended = now_ns();
            Ok(json!({
                "event": "done", "workload": "cat", "bytes": bytes,
                "started_ns": started, "time_ns": ended,
            }))
        }
        Workload::Keys { count, echo, line } => {
            let _mode = Mode::enter(*echo)?;
            if *line > 0 {
                out.write_all(b"\x1b[H\x1b[2J")?;
                out.flush()?;
            }
            let mut input = io::stdin().lock();
            let mut byte = [0_u8; 1];
            let mut read = 0_u64;
            while *count == 0 || read < *count {
                if input.read(&mut byte)? == 0 {
                    break;
                }
                let at = now_ns();
                read += 1;
                if *echo == Echo::Payload {
                    out.write_all(&byte)?;
                }
                if *line > 0 && read.is_multiple_of(*line) {
                    out.write_all(b"\r\x1b[K")?;
                }
                out.flush()?;
                events.tell(&json!({ "event": "key", "byte": byte[0], "time_ns": at }));
            }
            Ok(json!({ "event": "done", "workload": "keys", "count": read, "time_ns": now_ns() }))
        }
    }
}

/// The FIFO that the events go through, a line of JSON each, if one.
struct Events(Option<File>);

impl Events {
    /// The FIFO at `path`, opened to write, which waits for the tool to
    /// open it to read; none of no path.
    fn open(path: Option<&std::path::Path>) -> io::Result<Self> {
        path.map(|path| std::fs::OpenOptions::new().write(true).open(path))
            .transpose()
            .map(Self)
    }

    /// Tells `event`, if a FIFO takes it.
    fn tell(&mut self, event: &Value) {
        if let Some(fifo) = self.0.as_mut() {
            let _ = writeln!(fifo, "{event}");
            let _ = fifo.flush();
        }
    }
}

/// The grid of the terminal that the payload's output goes to: its
/// columns and rows, none of no terminal.
fn grid() -> (u16, u16) {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `size` is a writable `winsize` that `TIOCGWINSZ` fills in.
    let status = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &raw mut size) };
    if status == 0 {
        (size.ws_col, size.ws_row)
    } else {
        (0, 0)
    }
}

/// The terminal's input mode while it lives, as it was after.
struct Mode(libc::termios);

impl Mode {
    /// The input's terminal read a byte at a time: in raw mode for the
    /// payload's echo, or of the line discipline's echo and signals.
    fn enter(echo: Echo) -> io::Result<Self> {
        // SAFETY: `termios` is plain data, which `tcgetattr` fills in.
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: `saved` is a writable `termios` for `tcgetattr`.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &raw mut saved) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut mode = saved;
        match echo {
            // SAFETY: `mode` is a valid `termios`, which `cfmakeraw`
            // changes.
            Echo::Payload => unsafe { libc::cfmakeraw(&raw mut mode) },
            Echo::Tty => {
                mode.c_lflag &= !libc::ICANON;
                mode.c_lflag |= libc::ECHO;
                mode.c_cc[libc::VMIN] = 1;
                mode.c_cc[libc::VTIME] = 0;
            }
        }
        // SAFETY: `mode` is a valid `termios` for `tcsetattr`.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const mode) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(saved))
    }
}

impl Drop for Mode {
    fn drop(&mut self) {
        // SAFETY: the saved `termios` of the same terminal.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const self.0) };
    }
}
