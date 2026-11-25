//! `ttybench-startup`: how fast a terminal starts. Each round launches the
//! terminal running the payload, and measures, from the launch, the time
//! to its window on screen, as the window server lists it, and to the
//! payload's `ready`, once the terminal ran it; then quits it, and pauses.
//!
//! Of `--floor`, each round launches `ttybench-floor` too, the least app
//! that shows a window, as the terminal launches, by `open` of its bundle
//! or of its executable, and measures it to its window alike, before the
//! terminal in a round and after it in the next: its time is the floor of
//! the Mac's state that round, which the terminal's window less it, the
//! gap, leaves out.
//!
//! ```text
//! ttybench-startup --terminal <name> [--terminals <file>] [--feature <name>] [--rounds <n>]
//!                  [--pause <seconds>] [--season <n>] [--floor [--floor-bundle <app>]]
//!                  [--session <folder>]
//! ```

use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use ttybench::cli::{self, Args};
use ttybench::clock::now_ns;
use ttybench::config::Terminal;
use ttybench::payload::{self, Payload};
use ttybench::results::{self, Record, Session};
use ttybench::{floor, stats, terminal};

/// How long a round waits for the window and the payload.
const TIMEOUT: Duration = Duration::from_secs(30);

/// How often a round looks for the window.
const POLL: Duration = Duration::from_millis(5);

/// How long a terminal takes to quit, at most.
const QUIT: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-startup: {why}");
            ExitCode::FAILURE
        }
    }
}

/// A round's times from the launch, in milliseconds: to the window, and to
/// the payload's `ready`; the grid that the payload got; and the floor's
/// time to its window, of `--floor`.
struct Round {
    window_ms: f64,
    ready_ms: f64,
    grid: (u64, u64),
    floor_ms: Option<f64>,
}

fn run() -> Result<(), String> {
    let args = Args::parse(
        std::env::args().skip(1),
        &[
            "terminal",
            "terminals",
            "feature",
            "rounds",
            "pause",
            "season",
            "session",
            "floor-bundle",
        ],
        &["floor"],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let rounds: usize = args.number("rounds", 10)?;
    let pause = Duration::from_secs_f64(args.number("pause", 2.0)?);
    let season: usize = args.number("season", 0)?;
    let session = Session::open(std::path::Path::new(
        args.value("session").unwrap_or("ttybench-session"),
    ))
    .map_err(|error| format!("the session's folder: {error}"))?;
    let binary = payload::binary()?;
    let home = std::env::home_dir().unwrap_or_else(std::env::temp_dir);
    let floor = if args.flag("floor") {
        let bundle = match args.value("floor-bundle") {
            Some(bundle) => std::path::PathBuf::from(bundle),
            None => floor::cached_bundle(&floor::binary()?)
                .map_err(|error| format!("the floor's bundle: {error}"))?,
        };
        Some(floor::terminal_of(&terminal, &bundle))
    } else {
        if args.value("floor-bundle").is_some() {
            return Err("--floor-bundle takes --floor".to_owned());
        }
        None
    };
    let mut measured = Vec::new();
    for at in 0..season + rounds {
        let mut floor_ms = None;
        // The floor before the terminal in even rounds and after it in odd
        // ones, so that neither is always the first after a pause.
        let floor_first = at % 2 == 0;
        if let (Some(floor), true) = (&floor, floor_first) {
            floor_ms = Some(floor_round(floor, &binary, &home)?);
            thread::sleep(pause);
        }
        let mut round = round(&name, &terminal, &binary, &home)?;
        if let (Some(floor), false) = (&floor, floor_first) {
            thread::sleep(pause);
            floor_ms = Some(floor_round(floor, &binary, &home)?);
        }
        round.floor_ms = floor_ms;
        let seasoning = at < season;
        eprintln!(
            "{name} {} {}/{}: window {:.1} ms, ready {:.1} ms{}",
            if seasoning { "season" } else { "round" },
            if seasoning { at + 1 } else { at - season + 1 },
            if seasoning { season } else { rounds },
            round.window_ms,
            round.ready_ms,
            round
                .floor_ms
                .map_or_else(String::new, |ms| format!(", floor {ms:.1} ms"))
        );
        if !seasoning {
            measured.push(round);
        }
        thread::sleep(pause);
    }
    let version = results::app_version(&terminal.app);
    let grid = measured.last().map_or((0, 0), |round| round.grid);
    let mut conditions = serde_json::Map::new();
    if let Some(feature) = cli::feature(&args) {
        conditions.insert("feature".into(), feature.into());
    }
    conditions.insert("rounds".into(), rounds.into());
    conditions.insert("season".into(), season.into());
    conditions.insert("pause_s".into(), pause.as_secs_f64().into());
    conditions.insert("cols".into(), grid.0.into());
    conditions.insert("rows".into(), grid.1.into());
    let mut metrics = vec![
        (
            "window_ms",
            measured
                .iter()
                .map(|round| round.window_ms)
                .collect::<Vec<_>>(),
        ),
        (
            "ready_ms",
            measured.iter().map(|round| round.ready_ms).collect(),
        ),
    ];
    if floor.is_some() {
        metrics.push((
            "floor_window_ms",
            measured.iter().filter_map(|round| round.floor_ms).collect(),
        ));
        metrics.push((
            "window_gap_ms",
            measured
                .iter()
                .filter_map(|round| round.floor_ms.map(|floor| round.window_ms - floor))
                .collect(),
        ));
    }
    for (metric, samples) in metrics {
        if let Some(summary) = stats::summary(&samples) {
            println!(
                "{name} {metric}: median {:.1} ms, p95 {:.1} ms, of {} rounds",
                summary.median, summary.p95, summary.count
            );
        }
        session
            .append(&Record {
                tool: "startup".into(),
                terminal: name.clone(),
                version: version.clone(),
                metric: metric.into(),
                unit: "ms".into(),
                samples,
                conditions: conditions.clone(),
                cycle: None,
            })
            .map_err(|error| format!("the session's records: {error}"))?;
    }
    Ok(())
}

/// A round: the terminal launched running the payload at rest, its window
/// and its `ready` waited for, then quit.
fn round(
    name: &str,
    terminal: &Terminal,
    binary: &std::path::Path,
    home: &std::path::Path,
) -> Result<Round, String> {
    let payload =
        Payload::new(binary, &["rest"]).map_err(|error| format!("the payload: {error}"))?;
    let started = now_ns();
    let launched = terminal::launch(name, terminal, payload.command(), home, &[])
        .map_err(|error| format!("{name} does not launch: {error}"))?;
    let mut window = None;
    let mut ready = None;
    let deadline = started + u64::try_from(TIMEOUT.as_nanos()).unwrap_or(u64::MAX);
    while (window.is_none() || ready.is_none()) && now_ns() < deadline {
        if window.is_none() && launched.window().is_some() {
            window = Some(now_ns());
        }
        if ready.is_none()
            && let Some(event) = payload.next(POLL)
            && event["event"] == "ready"
        {
            ready = Some(event);
        }
    }
    launched
        .quit(QUIT)
        .map_err(|error| format!("{name} does not quit: {error}"))?;
    let (Some(window), Some(ready)) = (window, ready) else {
        return Err(format!(
            "{name} showed {} within {} s",
            if window.is_none() {
                "no window"
            } else {
                "no ready payload"
            },
            TIMEOUT.as_secs()
        ));
    };
    let ms = |at: u64| at.saturating_sub(started) as f64 / 1e6;
    Ok(Round {
        window_ms: ms(window),
        ready_ms: ms(ready["time_ns"].as_u64().unwrap_or(0)),
        grid: (
            ready["cols"].as_u64().unwrap_or(0),
            ready["rows"].as_u64().unwrap_or(0),
        ),
        floor_ms: None,
    })
}

/// A round of the floor `floor`, launched as the terminal is, of a payload
/// that it never runs, so that a launch by `open` gives it a script as the
/// terminal's does: the time from the launch to its window, in
/// milliseconds; then quit.
fn floor_round(
    floor: &Terminal,
    binary: &std::path::Path,
    home: &std::path::Path,
) -> Result<f64, String> {
    let payload =
        Payload::new(binary, &["rest"]).map_err(|error| format!("the payload: {error}"))?;
    let started = now_ns();
    let launched = terminal::launch("floor", floor, payload.command(), home, &[])
        .map_err(|error| format!("the floor does not launch: {error}"))?;
    // The window looked for as often as the terminal's is.
    let deadline = started + u64::try_from(TIMEOUT.as_nanos()).unwrap_or(u64::MAX);
    let mut window = None;
    while window.is_none() && now_ns() < deadline {
        if launched.window().is_some() {
            window = Some(now_ns());
        } else {
            thread::sleep(POLL);
        }
    }
    launched
        .quit(QUIT)
        .map_err(|error| format!("the floor does not quit: {error}"))?;
    let window = window
        .ok_or_else(|| format!("the floor showed no window within {} s", TIMEOUT.as_secs()))?;
    Ok(window.saturating_sub(started) as f64 / 1e6)
}
