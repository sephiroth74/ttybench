//! `ttybench-latency`: how fast a terminal shows a typed key. Each round
//! launches the terminal running the payload, which reads keys on an empty
//! row that the terminal's line discipline echoes them on, as `cat` at a
//! prompt; captures the terminal's window; finds the typed line of a few
//! keys; then strikes letters a spacing apart, and measures, from each
//! key's press to the first frame that the window server displayed of the
//! line changed, the key's latency.
//!
//! ```text
//! ttybench-latency --terminal <name> [--terminals <file>] [--keys <n>]
//!                  [--spacing <ms>] [--to process|system] [--rounds <n>]
//!                  [--pause <seconds>] [--session <folder>]
//! ```
//!
//! The keys go to the terminal's process, whichever app is in front, or,
//! of `--to system`, through the window server as a keyboard's, to the app
//! in front, the terminal as it launched. The capture takes the Screen
//! Recording permission, and the keys the Accessibility permission, of the
//! app that runs the tool.

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use ttybench_core::capture::Capture;
use ttybench_core::cli::{self, Args};
use ttybench_core::config::Terminal;
use ttybench_core::frames::{self, Frame, Line, Watch};
use ttybench_core::keys::{self, To};
use ttybench_core::payload::{self, Payload};
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

/// The letters struck in turn, which every layout of Latin letters types
/// as letters, whichever its keys.
const LETTERS: &str = "fjdkslaghrueiwoqptyznvmcbx";

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
    target: Target,
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
            "keys",
            "spacing",
            "to",
            "rounds",
            "pause",
            "session",
        ],
        &[],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let settings = Settings {
        keys: args.number("keys", 100)?,
        spacing: Duration::from_secs_f64(args.number("spacing", 150.0)? / 1000.0),
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
    conditions.insert("echo".into(), "tty".into());
    conditions.insert("cols".into(), grid.0.into());
    conditions.insert("rows".into(), grid.1.into());
    if let Some(refresh) = Machine::this().refresh_hz {
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
    let line = LINE.to_string();
    let payload = Payload::new(binary, &["keys", "--count", "0", "--line", &line])
        .map_err(|error| format!("the payload: {error}"))?;
    let (launched, ready) = terminal::launch_ready(name, terminal, &payload, home, TIMEOUT)?;
    let measured = measure(name, &launched, &ready, settings);
    launched
        .quit(QUIT)
        .map_err(|error| format!("{name} does not quit: {error}"))?;
    measured
}

/// The keys of a terminal launched, its payload `ready`, measured.
fn measure(
    name: &str,
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
    let capture = Capture::start(window.id)?;
    let mut typed = 0;
    let mut last = capture.next(Duration::from_secs(2));
    last = gather(&capture, last, Instant::now() + Duration::from_millis(200));
    let line = find_line(name, &capture, to, &mut typed, &mut last)?;
    capture.band(Some(line.band));
    last = gather(&capture, last, Instant::now() + Duration::from_millis(200));
    let changes = changes(&capture, &mut last, &line, QUIET);
    if changes > 0 {
        return Err(format!(
            "{name}'s typed line changed {changes} times in {} s of no key, as a blinking \
             cursor's does: turn its cursor's blinking off",
            QUIET.as_secs()
        ));
    }
    let mut latencies = Vec::with_capacity(settings.keys);
    let mut delays = Vec::with_capacity(settings.keys);
    let mut missed = 0;
    let mut next = Instant::now();
    for _ in 0..settings.keys {
        last = gather(&capture, last, next);
        let struck = Instant::now();
        let key_ns = keys::strike(to, letter(typed));
        typed += 1;
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
        next = (struck + settings.spacing).max(Instant::now() + AFTER);
    }
    Ok(Round {
        latencies,
        missed,
        delays,
        grid,
    })
}

/// The key of the letter struck `typed`-th.
fn letter(typed: usize) -> u16 {
    let letters = LETTERS.as_bytes();
    let letter = char::from(letters[typed % letters.len()]);
    keys::code(&letter.to_string()).map_or(0, |(code, _)| code)
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
    typed: &mut usize,
    last: &mut Option<Frame>,
) -> Result<Line, String> {
    let mut boxes = Vec::with_capacity(FINDING);
    for _ in 0..FINDING {
        let before = last
            .clone()
            .ok_or("the capture gave no frame of the window")?;
        let struck = Instant::now();
        keys::strike(to, letter(*typed));
        *typed += 1;
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
    fn every_letter_struck_has_its_own_key() {
        let mut codes: Vec<u16> = (0..LETTERS.len()).map(letter).collect();
        assert_eq!(letter(LETTERS.len()), letter(0));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 26);
    }
}
