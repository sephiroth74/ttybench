//! `ttybench-latency`: how fast a terminal shows a typed key. Each round
//! launches the terminal running the payload, which reads keys on the first
//! row of a cleared screen, which the terminal's line discipline echoes, as
//! `cat` at a prompt; captures the terminal's window; finds the typed line
//! of a few keys; then strikes `.`, as Typometer does, a spacing
//! apart, and measures, from each
//! key's press to the first frame that the window server displayed of the
//! line changed, the key's latency.
//!
//! ```text
//! ttybench-latency --terminal <name> [--terminals <file>] [--feature <name>] [--keys <n>]
//!                  [--spacing <ms>] [--sync] [--flood] [--to process|system]
//!                  [--rounds <n>] [--pause <seconds>] [--session <folder>]
//! ```
//!
//! Each spacing gets a random share of a display's refresh more, so that
//! the keys fall at every phase of the refresh alike, and their latencies
//! hold its whole range. Of `--sync`, each key comes a spacing after the
//! frame of the last one's character, as Typometer's synchronous mode
//! types them: at one phase of the refresh, of a spacing of whole
//! refreshes, which its latencies hold alone.
//!
//! Of `--flood`, a pane beside the measured one prints without a pause:
//! the terminal's `beside` keys open it, its `back` keys or its `click`
//! give the keyboard back, and a shell's `yes` runs in it once the typed
//! line is found.
//!
//! The keys go to the terminal's process, whichever app is in front, or,
//! of `--to system`, through the window server as a keyboard's, to the app
//! in front, the terminal as it launched. The capture takes the Screen
//! Recording permission, and the keys the Accessibility permission, of the
//! app that runs the tool.

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use ttybench_core::capture::{Capture, Source};
use ttybench_core::cli::{self, Args};
use ttybench_core::config::Terminal;
use ttybench_core::frames::{self, Frame, Line, Watch};
use ttybench_core::keys::{self, To};
use ttybench_core::payload::{self, Payload};
use ttybench_core::presence::Presence;
use ttybench_core::results::{self, Machine, Record, Session};
use ttybench_core::{stats, terminal};

/// How long a round waits for the terminal's window and its payload.
const TIMEOUT: Duration = Duration::from_secs(30);

/// How long a terminal takes to quit, at most.
const QUIT: Duration = Duration::from_secs(5);

/// How long the window rests once it shows, before its capture, for its
/// opening's animation to end.
const SETTLE: Duration = Duration::from_secs(1);

/// The keys that a line of the payload's takes before it is cleared.
const LINE: usize = 40;

/// The keys that find the typed line, and their spacing.
const FINDING: usize = 6;
const FINDING_SPACING: Duration = Duration::from_millis(300);

/// How long the typed line is watched of no key, for a change of its own,
/// as a blinking cursor's.
const QUIET: Duration = Duration::from_secs(1);

/// How long a key's character is waited for.
const ECHO: Duration = Duration::from_secs(1);

/// The rest after a key's character showed, at least, before the next key,
/// for the frames that follow it, as a cursor's slide.
const AFTER: Duration = Duration::from_millis(30);

/// The character struck at every key, as Typometer strikes it, so that no
/// key measured draws a glyph for the first time: the key of `.` on the US
/// layout, which types `.` on most layouts, as Typometer's key does.
const LETTER: &str = ".";

/// How long the pane beside takes to start its shell, at most.
const BESIDE: Duration = Duration::from_millis(1500);

/// The refresh of a display that tells none, in hertz.
const REFRESH_HZ: f64 = 60.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-latency: {why}");
            ExitCode::FAILURE
        }
    }
}

/// Where the keys go, as `--to` says.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Process,
    System,
}

/// A round's settings.
struct Settings {
    keys: usize,
    spacing: Duration,
    /// The random share of a spacing more, at most, none of `--sync`.
    jitter: Duration,
    sync: bool,
    flood: bool,
    target: Target,
    source: Source,
    presence: bool,
}

/// A round's measures: each key's latency, in milliseconds, the keys
/// whose character did not show, the capture's delay after each frame's
/// display, in milliseconds, and the grid that the payload got.
struct Round {
    latencies: Vec<f64>,
    missed: usize,
    delays: Vec<f64>,
    grid: (u64, u64),
}

fn run() -> Result<(), String> {
    let args = Args::parse(
        std::env::args().skip(1),
        &[
            "terminal",
            "terminals",
            "feature",
            "keys",
            "spacing",
            "to",
            "rounds",
            "pause",
            "session",
            "source",
            "presence",
        ],
        &["sync", "flood"],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    keys::may_post()?;
    let refresh_hz = Machine::this().refresh_hz;
    let sync = args.flag("sync");
    let flood = args.flag("flood");
    if flood && terminal.beside.is_none() {
        return Err(format!(
            "the terminals' file gives {name} no `beside` keys, of the pane that floods"
        ));
    }
    let settings = Settings {
        keys: args.number("keys", 100)?,
        spacing: Duration::from_secs_f64(args.number("spacing", 150.0)? / 1000.0),
        jitter: if sync {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(1.0 / refresh_hz.unwrap_or(REFRESH_HZ))
        },
        sync,
        flood,
        source: source(&args)?,
        presence: presence(&args)?,
        target: match args.value("to").unwrap_or("process") {
            "process" => Target::Process,
            "system" => Target::System,
            other => return Err(format!("--to takes process or system, not `{other}`")),
        },
    };
    let rounds: usize = args.number("rounds", 1)?;
    let pause = Duration::from_secs_f64(args.number("pause", 2.0)?);
    let session = Session::open(std::path::Path::new(
        args.value("session").unwrap_or("ttybench-session"),
    ))
    .map_err(|error| format!("the session's folder: {error}"))?;
    let binary = payload::binary()?;
    let home = std::env::home_dir().unwrap_or_else(std::env::temp_dir);
    let mut measured = Vec::new();
    for at in 0..rounds {
        let round = round(&name, &terminal, &binary, &home, &settings)?;
        let summary = stats::summary(&round.latencies);
        eprintln!(
            "{name} round {}/{rounds}: {} keys, median {:.1} ms, p95 {:.1} ms, {} missed; \
             the capture {:.1} ms after the display",
            at + 1,
            round.latencies.len(),
            summary.map_or(f64::NAN, |summary| summary.median),
            summary.map_or(f64::NAN, |summary| summary.p95),
            round.missed,
            stats::summary(&round.delays).map_or(f64::NAN, |summary| summary.median),
        );
        measured.push(round);
        if at + 1 < rounds {
            thread::sleep(pause);
        }
    }
    let version = results::app_version(&terminal.app);
    let grid = measured.last().map_or((0, 0), |round| round.grid);
    let mut conditions = serde_json::Map::new();
    if let Some(feature) = cli::feature(&args) {
        conditions.insert("feature".into(), feature.into());
    }
    conditions.insert("rounds".into(), rounds.into());
    conditions.insert("keys".into(), settings.keys.into());
    conditions.insert(
        "spacing_ms".into(),
        (settings.spacing.as_secs_f64() * 1000.0).into(),
    );
    conditions.insert(
        "to".into(),
        match settings.target {
            Target::Process => "process",
            Target::System => "system",
        }
        .into(),
    );
    conditions.insert(
        "phase".into(),
        if settings.sync { "sync" } else { "random" }.into(),
    );
    conditions.insert(
        "jitter_ms".into(),
        (settings.jitter.as_secs_f64() * 1000.0).into(),
    );
    conditions.insert(
        "beside".into(),
        if settings.flood { "flood" } else { "rest" }.into(),
    );
    conditions.insert("echo".into(), "tty".into());
    conditions.insert("presence".into(), settings.presence.into());
    conditions.insert(
        "source".into(),
        match settings.source {
            Source::Screen => "screen",
            Source::Window => "window",
        }
        .into(),
    );
    conditions.insert("cols".into(), grid.0.into());
    conditions.insert("rows".into(), grid.1.into());
    if let Some(refresh) = refresh_hz {
        conditions.insert("refresh_hz".into(), refresh.into());
    }
    let latencies: Vec<f64> = measured
        .iter()
        .flat_map(|round| round.latencies.iter().copied())
        .collect();
    if let Some(summary) = stats::summary(&latencies) {
        println!(
            "{name} latency_ms: median {:.1} ms, p95 {:.1} ms, p99 {:.1} ms, of {} keys",
            summary.median, summary.p95, summary.p99, summary.count
        );
    }
    for (metric, unit, samples) in [
        ("latency_ms", "ms", latencies),
        (
            "missed_keys",
            "keys",
            measured.iter().map(|round| round.missed as f64).collect(),
        ),
    ] {
        session
            .append(&Record {
                tool: "latency".into(),
                terminal: name.clone(),
                version: version.clone(),
                metric: metric.into(),
                unit: unit.into(),
                samples,
                conditions: conditions.clone(),
                cycle: None,
            })
            .map_err(|error| format!("the session's records: {error}"))?;
    }
    Ok(())
}

/// A round: the terminal launched running the payload's keys, measured,
/// then quit.
fn round(
    name: &str,
    terminal: &Terminal,
    binary: &std::path::Path,
    home: &std::path::Path,
    settings: &Settings,
) -> Result<Round, String> {
    let _presence = settings.presence.then(Presence::start);
    let line = LINE.to_string();
    let payload = Payload::new(binary, &["keys", "--count", "0", "--line", &line])
        .map_err(|error| format!("the payload: {error}"))?;
    let (launched, ready) = terminal::launch_ready(name, terminal, &payload, home, TIMEOUT)?;
    let measured = measure(name, terminal, &launched, &ready, settings);
    launched
        .quit(QUIT)
        .map_err(|error| format!("{name} does not quit: {error}"))?;
    measured
}

/// The keys of a terminal launched, its payload `ready`, measured.
fn measure(
    name: &str,
    terminal: &Terminal,
    launched: &terminal::Launched,
    ready: &serde_json::Value,
    settings: &Settings,
) -> Result<Round, String> {
    let grid = (
        ready["cols"].as_u64().unwrap_or(0),
        ready["rows"].as_u64().unwrap_or(0),
    );
    if grid.0 < LINE as u64 + 2 {
        return Err(format!(
            "{name}'s window has {} columns, of {} at least",
            grid.0,
            LINE + 2
        ));
    }
    launched
        .wait_window(TIMEOUT)
        .ok_or_else(|| format!("{name} showed no window"))?;
    thread::sleep(SETTLE);
    let window = launched
        .window()
        .ok_or_else(|| format!("{name}'s window went"))?;
    let to = match settings.target {
        Target::Process => To::Process(window.pid),
        Target::System => To::System,
    };
    let flood = match (&terminal.beside, settings.flood) {
        (Some(beside), true) => Some(Flood::open(launched, window.pid, beside)?),
        _ => None,
    };
    let capture = Capture::start(window.id, settings.source)?;
    let mut last = capture.next(Duration::from_secs(2));
    last = gather(&capture, last, Instant::now() + Duration::from_millis(200));
    let line = find_line(name, &capture, to, &mut last)?;
    if let Some(flood) = &flood {
        flood.start(name, launched)?;
    }
    capture.band(Some(line.band));
    last = gather(&capture, last, Instant::now() + Duration::from_millis(200));
    let changes = changes(&capture, &mut last, &line, QUIET);
    if changes > 0 {
        return Err(format!(
            "{name}'s typed line changed {changes} times in {} s of no key, as a blinking \
             cursor's does, or a pane beside that reaches its row: turn its cursor's blinking \
             off",
            QUIET.as_secs()
        ));
    }
    let mut random = Random::new(ttybench_core::clock::now_ns());
    let mut latencies = Vec::with_capacity(settings.keys);
    let mut delays = Vec::with_capacity(settings.keys);
    let mut missed = 0;
    let mut next = Instant::now();
    for _ in 0..settings.keys {
        last = gather(&capture, last, next);
        let struck = Instant::now();
        let key_ns = keys::strike(to, letter());
        let mut watch = Watch::new(key_ns, line.band, last.take());
        let deadline = Instant::now() + ECHO;
        let mut shown = None;
        while shown.is_none() {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let Some(frame) = capture.next(left) else {
                break;
            };
            let arrived = frame.arrived_ns.saturating_sub(frame.display_ns);
            if let Some(at) = watch.take(frame) {
                shown = Some(at);
                delays.push(arrived as f64 / 1e6);
            }
        }
        last = watch.into_last();
        match shown {
            Some(at) => latencies.push(at.saturating_sub(key_ns) as f64 / 1e6),
            None => missed += 1,
        }
        let after = match shown {
            // A spacing after the frame of the character, on the shared
            // clock, as Instant counts the same time.
            Some(at) if settings.sync => {
                let now = ttybench_core::clock::now_ns();
                let due = at + u64::try_from(settings.spacing.as_nanos()).unwrap_or(u64::MAX);
                Instant::now() + Duration::from_nanos(due.saturating_sub(now))
            }
            _ => struck + settings.spacing + settings.jitter.mul_f64(random.share()),
        };
        next = after.max(Instant::now() + AFTER);
    }
    if missed * 2 > settings.keys {
        return Err(format!(
            "{missed} of {} keys showed nothing on {name}'s typed line: did the pane beside \
             keep the keyboard, or move the line?",
            settings.keys
        ));
    }
    drop(flood);
    Ok(Round {
        latencies,
        missed,
        delays,
        grid,
    })
}

/// A pane beside the measured one, of a shell that waits to print `yes`
/// without a pause until a file of its own shows: its script and that
/// file, in `/tmp`, so that its command types short.
struct Flood {
    script: std::path::PathBuf,
    go: std::path::PathBuf,
}

impl Flood {
    /// Opens the pane beside of `beside`'s keys in the terminal's process
    /// `pid`, its shell running the script that waits, and gives the
    /// keyboard back to the measured pane.
    fn open(
        launched: &terminal::Launched,
        pid: libc::pid_t,
        beside: &ttybench_core::config::Beside,
    ) -> Result<Self, String> {
        let stem = format!("/tmp/ttybench-flood-{}", std::process::id());
        let script = std::path::PathBuf::from(format!("{stem}.sh"));
        let go = std::path::PathBuf::from(format!("{stem}.go"));
        let _ = std::fs::remove_file(&go);
        std::fs::write(
            &script,
            format!(
                "until [ -e {} ]; do sleep 0.05; done\nexec yes\n",
                go.display()
            ),
        )
        .map_err(|error| format!("the flood's script: {error}"))?;
        let flood = Self { script, go };
        for key in &beside.open {
            launched.press(key);
        }
        thread::sleep(BESIDE);
        keys::type_text(pid, &format!("sh {}", flood.script.display()));
        if let Ok(enter) = ttybench_core::config::Key::parse("enter") {
            keys::press(pid, &enter);
        }
        thread::sleep(Duration::from_millis(300));
        for key in &beside.back {
            launched.press(key);
        }
        if let (Some([x, y]), Some(window)) = (beside.click, launched.window()) {
            let frame = window.frame;
            keys::click(frame.x + x * frame.width, frame.y + y * frame.height);
        }
        thread::sleep(Duration::from_millis(500));
        Ok(flood)
    }

    /// Starts the flood, and waits for its `yes` among the terminal's
    /// processes.
    fn start(&self, name: &str, launched: &terminal::Launched) -> Result<(), String> {
        std::fs::write(&self.go, "").map_err(|error| format!("the flood's start: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let flooding = launched
                .tree()
                .iter()
                .any(|&pid| ttybench_core::process::name(pid).is_ok_and(|name| name == "yes"));
            if flooding {
                thread::sleep(Duration::from_millis(500));
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(format!(
            "{name}'s pane beside runs no `yes`: do its `beside` keys open a pane of a shell \
             and give it the keyboard?"
        ))
    }
}

impl Drop for Flood {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.script);
        let _ = std::fs::remove_file(&self.go);
    }
}

/// Random shares of a whole, of xorshift64*, as no crate is needed for a
/// key's spacing.
struct Random(u64);

impl Random {
    /// The shares of the seed `seed`.
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// The next share, from 0 to 1.
    fn share(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1_u64 << 53) as f64
    }
}

/// Whether the user is declared active while the tool measures, as
/// `--presence` says: on by default.
fn presence(args: &Args) -> Result<bool, String> {
    match args.value("presence").unwrap_or("on") {
        "on" => Ok(true),
        "off" => Ok(false),
        other => Err(format!("--presence takes on or off, not `{other}`")),
    }
}

/// The capture's source, as `--source` says: the window by default.
fn source(args: &Args) -> Result<Source, String> {
    match args.value("source").unwrap_or("window") {
        "screen" => Ok(Source::Screen),
        "window" => Ok(Source::Window),
        other => Err(format!("--source takes screen or window, not `{other}`")),
    }
}

/// The key of the letter struck.
fn letter() -> u16 {
    keys::code(LETTER).map_or(0, |(code, _)| code)
}

/// The frames that come until `until`, the last of them, or `last` of
/// none.
fn gather(capture: &Capture, mut last: Option<Frame>, until: Instant) -> Option<Frame> {
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        match capture.next(left) {
            Some(frame) => last = Some(frame),
            None => break,
        }
    }
    while let Some(frame) = capture.try_next() {
        last = Some(frame);
    }
    last
}

/// The typed line, of [`FINDING`] keys struck slowly, each key's change the
/// box of what differs between the frames before and after it.
fn find_line(
    name: &str,
    capture: &Capture,
    to: To,
    last: &mut Option<Frame>,
) -> Result<Line, String> {
    let mut boxes = Vec::with_capacity(FINDING);
    for _ in 0..FINDING {
        let before = last
            .clone()
            .ok_or("the capture gave no frame of the window")?;
        let struck = Instant::now();
        keys::strike(to, letter());
        *last = gather(capture, last.take(), struck + FINDING_SPACING);
        let after = last.as_ref().unwrap_or(&before);
        match frames::changed_box(&before, after, None) {
            Some(changed) => boxes.push(changed),
            None => {
                return Err(format!(
                    "a key struck showed nothing in {name}'s window: is the terminal in front, \
                     as keys to the system need it, and does the app that runs the tool have \
                     the Accessibility permission?"
                ));
            }
        }
    }
    let (width, height) = capture.size();
    frames::line_of(&boxes, LINE, width, height)
        .map_err(|why| format!("{name}'s typed line was not found: {why}"))
}

/// The frames within `duration` whose `line` changed from the frame
/// before, as of no key.
fn changes(capture: &Capture, last: &mut Option<Frame>, line: &Line, duration: Duration) -> usize {
    let until = Instant::now() + duration;
    let mut changes = 0;
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        let Some(frame) = capture.next(left) else {
            break;
        };
        if let Some(before) = last.as_ref()
            && frames::changed_box(before, &frame, Some(line.band)).is_some()
        {
            changes += 1;
        }
        *last = Some(frame);
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_character_struck_has_its_key() {
        assert_eq!(letter(), 0x2F);
    }

    #[test]
    fn the_shares_fall_alike_over_the_whole() {
        let mut random = Random::new(42);
        let mut tenths = [0; 10];
        for _ in 0..10_000 {
            let share = random.share();
            assert!((0.0..1.0).contains(&share));
            tenths[(share * 10.0) as usize] += 1;
        }
        assert!(
            tenths.iter().all(|&count| (850..1150).contains(&count)),
            "{tenths:?}"
        );
    }
}
