//! `ttybench-run`: the coordinator, which runs a session that a file
//! describes: its terminals, its tools of their arguments, its cycles,
//! each run's terminals in an order that turns at each cycle, so that each
//! is first as often as last, and a pause before each run. Each tool runs
//! as a process of its own, in a folder of its own in the session's, whose
//! records the coordinator gathers; a run that fails is told, and the
//! session goes on; `--resume` takes up a session that was cut off, of the
//! runs that did not end well. At its end, `ttybench report` writes the
//! session's report into its folder, then `ttybench charts` its charts,
//! unless `--no-report`.
//!
//! ```text
//! ttybench run <session.toml> [--folder <folder>] [--dry-run] [--no-report]
//! ttybench run --resume <folder> [--no-report]
//! ```

mod log;
mod session;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{Entry, Status};
use session::{Session, Step};
use ttybench_core::cli::{self, Args};
use ttybench_core::config::{self, Terminals, expand_home};
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Load, RECORDS};
use ttybench_core::style::{self, Style};
use ttybench_core::{process, terminal, tools};

/// The copy of the session's file in its folder, which `--resume` reads.
const SESSION: &str = "session.toml";

/// The copy of the terminals' file in the session's folder, which its
/// tools read.
const TERMINALS: &str = "terminals.toml";

/// The file of a run's output, in its folder.
const OUTPUT: &str = "output.txt";

/// How many of a failed run's last lines its entry keeps.
const TAIL: usize = 8;

/// How often `sudo`'s credentials are renewed.
const SUDO_EVERY: Duration = Duration::from_secs(60);

/// The pause between two launches of a terminal's seasoning.
const SEASON_PAUSE: Duration = Duration::from_secs(1);

/// How long a launch of the seasoning waits for its terminal.
const SEASON_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a run's output is read after its tool exits.
const DRAIN: Duration = Duration::from_secs(1);

/// How often a run's tool is looked at.
const STEP: Duration = Duration::from_millis(100);

/// How the coordinator runs.
const USAGE: &str = "ttybench run <session.toml> [--folder <folder>] [--dry-run] [--no-report]\n       ttybench run --resume <folder> [--no-report]";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(why) => {
            eprintln!("ttybench-run: {why}");
            ExitCode::from(2)
        }
    }
}

/// A session as the coordinator takes it: of its file, or of the copy in
/// its folder that it resumes.
struct Opened {
    /// The session.
    session: Session,
    /// Its file's text, which the folder keeps.
    text: String,
    /// The terminals' file's text, which the folder keeps.
    terminals_text: String,
    /// The style's file's text, checked, which the folder keeps, if the
    /// session names one.
    style_text: Option<String>,
    /// The terminals.
    terminals: Terminals,
    /// The session's folder.
    folder: PathBuf,
    /// Whether it resumes a session cut off.
    resume: bool,
}

impl Opened {
    /// The session of the file at `file`, in `folder` or the one that it
    /// names.
    fn new(file: &Path, folder: Option<&str>) -> Result<Self, String> {
        let text = std::fs::read_to_string(file)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let session: Session = config::parse(file, &text).map_err(|error| error.to_string())?;
        let terminals_path = session
            .terminals_path(file)
            .unwrap_or_else(|| cli::terminals_file(&Args::default()));
        let terminals_text = std::fs::read_to_string(&terminals_path)
            .map_err(|error| format!("{}: {error}", terminals_path.display()))?;
        let terminals =
            config::parse(&terminals_path, &terminals_text).map_err(|error| error.to_string())?;
        // The style checked before any run, kept with the session.
        let style_text = match session.style_path(file) {
            Some(path) => {
                Style::read(&path)?;
                Some(
                    std::fs::read_to_string(&path)
                        .map_err(|error| format!("{}: {error}", path.display()))?,
                )
            }
            None => None,
        };
        let folder = folder.map_or_else(
            || session.folder(file, &local_date()),
            |folder| PathBuf::from(expand_home(folder)),
        );
        Ok(Self {
            session,
            text,
            terminals_text,
            style_text,
            terminals,
            folder,
            resume: false,
        })
    }

    /// The session that `folder` holds, to take up.
    fn resume(folder: &Path) -> Result<Self, String> {
        let file = folder.join(SESSION);
        let text = std::fs::read_to_string(&file)
            .map_err(|error| format!("{} holds no session to resume: {error}", folder.display()))?;
        let session = config::parse(&file, &text).map_err(|error| error.to_string())?;
        let terminals_path = folder.join(TERMINALS);
        let terminals_text = std::fs::read_to_string(&terminals_path)
            .map_err(|error| format!("{}: {error}", terminals_path.display()))?;
        let terminals =
            config::parse(&terminals_path, &terminals_text).map_err(|error| error.to_string())?;
        // Its copy of the style, which the report reads of the folder.
        let style_text = None;
        Ok(Self {
            session,
            text,
            terminals_text,
            style_text,
            terminals,
            folder: folder.to_path_buf(),
            resume: true,
        })
    }
}

/// Runs the session that the arguments name: whether each of its runs
/// ended well.
fn run() -> Result<bool, String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(true);
    }
    let file = args
        .first()
        .is_some_and(|arg| !arg.starts_with("--"))
        .then(|| PathBuf::from(args.remove(0)));
    let options = Args::parse(args, &["folder", "resume"], &["dry-run", "no-report"])?;
    let opened = match (file, options.value("resume")) {
        (Some(file), None) => Opened::new(&file, options.value("folder"))?,
        (None, Some(_)) if options.value("folder").is_some() => {
            return Err("--resume takes the session's folder, of no --folder".to_owned());
        }
        (None, Some(folder)) => Opened::resume(Path::new(&expand_home(folder)))?,
        (Some(_), Some(_)) => {
            return Err(
                "--resume takes the session's folder alone, not a session's file".to_owned(),
            );
        }
        (None, None) => return Err(format!("which session?\n{USAGE}")),
    };
    let session = &opened.session;
    let known = opened.terminals.terminals.keys().cloned().collect();
    session.check(&known)?;
    let dirs = tools::search_dirs(
        std::env::current_exe().ok().as_deref(),
        std::env::var_os("PATH").as_deref(),
    );
    let mut binaries = BTreeMap::new();
    for run in &session.runs {
        let binary = tools::find(&run.tool, &dirs).ok_or_else(|| {
            format!(
                "there is no {}{} beside ttybench-run or on the PATH",
                tools::PREFIX,
                run.tool
            )
        })?;
        binaries.insert(run.tool.clone(), binary);
    }
    for warning in session.unbalanced() {
        eprintln!("ttybench-run: {warning}");
    }
    let steps = session.steps();
    let folder = &opened.folder;
    let lacks = |step: &Step| -> Option<String> {
        step.feature.clone().filter(|feature| {
            !opened.terminals.terminals[&step.terminal]
                .features
                .contains_key(feature)
        })
    };
    if options.flag("dry-run") {
        for (at, step) in steps.iter().enumerate() {
            let skipped = lacks(step).map_or(String::new(), |feature| {
                format!(" (skipped: no feature `{feature}`)")
            });
            println!(
                "{:>4}. cycle {}, {} of {}, {} of its order: ttybench {} {}{skipped}",
                at + 1,
                step.cycle,
                step.tool,
                step.terminal,
                step.position,
                step.tool,
                step.args.join(" ")
            );
        }
        println!("{} runs, into {}", steps.len(), folder.display());
        return Ok(true);
    }
    if !opened.resume {
        if folder.join(SESSION).exists() {
            return Err(format!(
                "{0} holds a session already, which `ttybench run --resume {0}` takes up",
                folder.display()
            ));
        }
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        let style = opened.style_text.as_ref().map(|text| (style::FILE, text));
        for (name, text) in [(SESSION, &opened.text), (TERMINALS, &opened.terminals_text)]
            .into_iter()
            .chain(style)
        {
            std::fs::write(folder.join(name), text)
                .map_err(|error| format!("{}: {error}", folder.join(name).display()))?;
        }
    }
    results::Session::open(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let entries = log::read(folder).map_err(|error| error.to_string())?;
    let done = log::done(&entries);
    let pending: Vec<&Step> = steps
        .iter()
        .filter(|step| !done.contains(&step.id))
        .collect();
    let report = |folder: &Path| {
        if !options.flag("no-report") {
            write_report(folder, &dirs);
        }
    };
    if pending.is_empty() {
        let count = log::gather(folder).map_err(|error| error.to_string())?;
        println!(
            "every run of {} has ended, of {count} records",
            folder.display()
        );
        report(folder);
        return Ok(true);
    }
    if opened.resume {
        eprintln!(
            "ttybench-run: {} of {} runs ended well before, {} to go",
            steps.len() - pending.len(),
            steps.len(),
            pending.len()
        );
    }
    let mut before = BTreeMap::new();
    for name in &session.terminals {
        let pids: BTreeSet<libc::pid_t> =
            terminal::running_copies(&opened.terminals.terminals[name].app)
                .into_iter()
                .collect();
        if !pids.is_empty() {
            eprintln!(
                "ttybench-run: {name} runs already ({pids:?}): its windows share the machine with the measures"
            );
        }
        before.insert(name.clone(), pids);
    }
    if pending.iter().any(|step| step.sudo) {
        keep_sudo()?;
    }
    if session.season > 0 {
        let names: BTreeSet<&str> = pending.iter().map(|step| step.terminal.as_str()).collect();
        let order = session
            .terminals
            .iter()
            .filter(|name| names.contains(name.as_str()));
        season(order, &opened.terminals, session.season)?;
    }
    let terminals_file = folder.join(TERMINALS);
    let pause = Duration::from_secs_f64(session.pause);
    let timeout = Duration::from_secs_f64(session.timeout);
    let (mut failed, mut skipped) = (0, 0);
    for (at, step) in pending.iter().enumerate() {
        eprintln!(
            "\nttybench-run: [{}/{}] cycle {}/{}: {} of {}, {} of its order",
            at + 1,
            pending.len(),
            step.cycle,
            session.cycles,
            step.tool,
            step.terminal,
            step.position
        );
        let entry = match lacks(step) {
            // A terminal that has not the run's feature measures nothing
            // alike: skipped, of no launch.
            Some(feature) => skip(step, &feature),
            None => {
                thread::sleep(pause);
                execute(
                    step,
                    &binaries[&step.tool],
                    folder,
                    &terminals_file,
                    timeout,
                )
            }
        };
        if entry.status == Status::Skipped {
            skipped += 1;
            eprintln!(
                "ttybench-run: {} skipped: {}",
                step.id,
                entry.error.as_deref().unwrap_or("")
            );
        }
        if entry.status == Status::Failed {
            failed += 1;
            eprintln!(
                "ttybench-run: {} failed: {}",
                step.id,
                entry.error.as_deref().unwrap_or("")
            );
            stop_left(
                &step.terminal,
                &opened.terminals.terminals[&step.terminal].app,
                &before[&step.terminal],
            );
        }
        log::append(folder, &entry).map_err(|error| format!("the session's log: {error}"))?;
        log::gather(folder).map_err(|error| format!("the session's records: {error}"))?;
    }
    let count = log::gather(folder).map_err(|error| format!("the session's records: {error}"))?;
    println!(
        "{} runs, {failed} failed, {skipped} skipped; {count} records in {}",
        pending.len(),
        folder.join(RECORDS).display()
    );
    if failed > 0 {
        println!(
            "`ttybench run --resume {}` runs the failed ones again",
            folder.display()
        );
    }
    report(folder);
    Ok(failed == 0)
}

/// Writes the report of the session in `folder` of `ttybench-report`,
/// then its charts of `ttybench-charts`, each found in `dirs`; one that
/// fails is told, the session's end unchanged.
fn write_report(folder: &Path, dirs: &[PathBuf]) {
    for tool in ["report", "charts"] {
        let Some(binary) = tools::find(tool, dirs) else {
            eprintln!(
                "ttybench-run: there is no {}{tool} beside ttybench-run or on the PATH: no {tool}",
                tools::PREFIX
            );
            return;
        };
        match Command::new(&binary)
            .arg(folder)
            .stdin(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("ttybench-run: the {tool} failed: {status}");
                return;
            }
            Err(error) => {
                eprintln!(
                    "ttybench-run: {}{tool} does not start: {error}",
                    tools::PREFIX
                );
                return;
            }
        }
    }
}

/// The entry of `step` skipped, as its terminal has not its run's feature
/// `feature`.
fn skip(step: &Step, feature: &str) -> Entry {
    Entry {
        id: step.id.clone(),
        cycle: step.cycle,
        run: step.run,
        position: step.position,
        tool: step.tool.clone(),
        terminal: step.terminal.clone(),
        args: step.args.clone(),
        started: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
        seconds: 0.0,
        status: Status::Skipped,
        code: None,
        error: Some(format!(
            "{} has no feature `{feature}` in the terminals' file",
            step.terminal
        )),
        load: None,
    }
}

/// Runs `step`'s tool of `binary` in its folder of the session in
/// `folder`, of the terminals of `terminals_file`, stopped after
/// `timeout`: its log's entry.
fn execute(
    step: &Step,
    binary: &Path,
    folder: &Path,
    terminals_file: &Path,
    timeout: Duration,
) -> Entry {
    let place = folder.join(log::RUNS).join(&step.id);
    let _ = std::fs::remove_dir_all(&place);
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let clock = Instant::now();
    let load = Load::starting();
    let mut args: Vec<OsString> = step.args.iter().map(OsString::from).collect();
    args.extend([
        "--terminals".into(),
        terminals_file.into(),
        "--terminal".into(),
        step.terminal.clone().into(),
        "--session".into(),
        place.clone().into(),
    ]);
    let watched = std::fs::create_dir_all(&place)
        .map_err(|error| format!("its folder: {error}"))
        .and_then(|()| watch(binary, &args, &place.join(OUTPUT), timeout));
    let (status, code, error) = match watched {
        Ok(Watched { code: Some(0), .. }) => (Status::Done, Some(0), None),
        Ok(Watched {
            code: Some(code),
            tail,
            ..
        }) if code == i32::from(ttybench_core::cli::SKIPPED) => {
            (Status::Skipped, Some(code), tail.into_iter().last())
        }
        Ok(Watched {
            code,
            tail,
            timed_out,
        }) => {
            let why = if timed_out {
                format!("it took more than {} s, and was stopped", timeout.as_secs())
            } else {
                code.map_or("it ended of a signal".to_owned(), |code| {
                    format!("it exited of {code}")
                })
            };
            let tail: Vec<String> = tail.into_iter().collect();
            let error = if tail.is_empty() {
                why
            } else {
                format!("{why}:\n{}", tail.join("\n"))
            };
            (Status::Failed, code, Some(error))
        }
        Err(why) => (Status::Failed, None, Some(why)),
    };
    Entry {
        id: step.id.clone(),
        cycle: step.cycle,
        run: step.run,
        position: step.position,
        tool: step.tool.clone(),
        terminal: step.terminal.clone(),
        args: step.args.clone(),
        started,
        seconds: clock.elapsed().as_secs_f64(),
        status,
        code,
        error,
        load: Some(Load::ending(load)),
    }
}

/// How a tool's process ended.
struct Watched {
    /// Its exit code, of none if a signal ended it.
    code: Option<i32>,
    /// Its output's last lines.
    tail: VecDeque<String>,
    /// Whether it took too long and was stopped.
    timed_out: bool,
}

/// Runs `binary` of `args`, its output shown and written to `output`,
/// stopped after `timeout`.
fn watch(
    binary: &Path,
    args: &[OsString],
    output: &Path,
    timeout: Duration,
) -> Result<Watched, String> {
    let mut file =
        std::fs::File::create(output).map_err(|error| format!("its output's file: {error}"))?;
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{} does not start: {error}", binary.display()))?;
    let (sender, lines) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        read_lines(stdout, false, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        read_lines(stderr, true, sender.clone());
    }
    drop(sender);
    let deadline = Instant::now() + timeout;
    let mut tail = VecDeque::new();
    let (mut open, mut code, mut ended, mut timed_out) = (true, None, None, false);
    loop {
        let message = if open {
            lines.recv_timeout(STEP)
        } else {
            thread::sleep(STEP);
            Err(RecvTimeoutError::Timeout)
        };
        match message {
            Ok((error, line)) => {
                if error {
                    eprintln!("{line}");
                } else {
                    println!("{line}");
                }
                let _ = writeln!(file, "{line}");
                tail.push_back(line);
                if tail.len() > TAIL {
                    tail.pop_front();
                }
            }
            Err(RecvTimeoutError::Disconnected) => open = false,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if ended.is_none() {
            if let Ok(Some(status)) = child.try_wait() {
                code = status.code();
                ended = Some(Instant::now());
            } else if Instant::now() >= deadline {
                let _ = child.kill();
                code = child.wait().ok().and_then(|status| status.code());
                ended = Some(Instant::now());
                timed_out = true;
            }
        }
        if ended.is_some_and(|at| !open || at.elapsed() >= DRAIN) {
            break;
        }
    }
    Ok(Watched {
        code,
        tail,
        timed_out,
    })
}

/// Sends the lines of `stream` through `sender`, each of whether it is
/// the error stream, from a thread of their own.
fn read_lines(
    stream: impl Read + Send + 'static,
    error: bool,
    sender: mpsc::Sender<(bool, String)>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&line)
                        .trim_end_matches(['\n', '\r'])
                        .to_owned();
                    if sender.send((error, text)).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Asks for `sudo` once, as a run needs it, and renews its credentials
/// for the session's length.
fn keep_sudo() -> Result<(), String> {
    eprintln!("ttybench-run: a run needs sudo, which the session asks for once");
    let status = Command::new("sudo")
        .arg("-v")
        .status()
        .map_err(|error| format!("sudo: {error}"))?;
    if !status.success() {
        return Err("sudo refused, which a run needs".to_owned());
    }
    thread::spawn(|| {
        loop {
            thread::sleep(SUDO_EVERY);
            let _ = Command::new("sudo")
                .args(["-n", "-v"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
    Ok(())
}

/// Launches each terminal of `names` `times` times, at rest, not counted.
fn season<'a>(
    names: impl Iterator<Item = &'a String>,
    terminals: &Terminals,
    times: u32,
) -> Result<(), String> {
    let binary = payload::binary()?;
    let home = std::env::home_dir().unwrap_or_else(std::env::temp_dir);
    for name in names {
        let terminal = &terminals.terminals[name];
        for at in 1..=times {
            eprintln!("ttybench-run: seasoning {name}, {at}/{times}");
            let payload = Payload::new(&binary, &["rest"])
                .map_err(|error| format!("the payload: {error}"))?;
            match terminal::launch_ready(name, terminal, &payload, &home, SEASON_TIMEOUT) {
                Ok((launched, _)) => {
                    if let Err(error) = launched.quit(Duration::from_secs(5)) {
                        eprintln!("ttybench-run: {name} does not quit: {error}");
                    }
                }
                Err(why) => {
                    eprintln!("ttybench-run: {name} did not start to season: {why}");
                    break;
                }
            }
            thread::sleep(SEASON_PAUSE);
        }
    }
    Ok(())
}

/// Stops the copies of the terminal `name` of `app` that a failed run
/// left, those that did not run before the session, `before`.
fn stop_left(name: &str, app: &Path, before: &BTreeSet<libc::pid_t>) {
    let left: Vec<libc::pid_t> = terminal::running_copies(app)
        .into_iter()
        .filter(|pid| !before.contains(pid))
        .collect();
    if left.is_empty() {
        return;
    }
    eprintln!("ttybench-run: stopping the copies of {name} that the run left, {left:?}");
    for &pid in &left {
        let _ = process::terminate(pid);
    }
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until && left.iter().any(|&pid| process::exists(pid)) {
        thread::sleep(STEP);
    }
    for &pid in left.iter().filter(|&&pid| process::exists(pid)) {
        let _ = process::kill(pid);
    }
}

/// The local date and time, as `2026-11-23-1830`.
fn local_date() -> String {
    // SAFETY: `time` of a null pointer only returns the time.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: `tm` is plain data, of which zeros are a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to live values of their types, which
    // `localtime_r` reads and writes alone.
    unsafe { libc::localtime_r(&now, &mut tm) };
    format!(
        "{:04}-{:02}-{:02}-{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}
