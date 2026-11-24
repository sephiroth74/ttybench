//! `ttybench-startup`: how fast a terminal starts. Each round launches the
//! terminal running the payload, and measures, from the launch, the time
//! to its window on screen, as the window server lists it, and to the
//! payload's `ready`, once the terminal ran it; then quits it, and pauses.
//!
//! ```text
//! ttybench-startup --terminal <name> [--terminals <file>] [--feature <name>] [--rounds <n>]
//!                  [--pause <seconds>] [--season <n>] [--session <folder>]
//! ```

use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use ttybench_core::cli::{self, Args};
use ttybench_core::clock::now_ns;
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Record, Session};
use ttybench_core::{stats, terminal};

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
/// the payload's `ready`; and the grid that the payload got.
struct Round {
    window_ms: f64,
    ready_ms: f64,
    grid: (u64, u64),
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
        ],
        &[],
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
    let mut measured = Vec::new();
    for at in 0..season + rounds {
        let round = round(&name, &terminal, &binary, &home)?;
        let seasoning = at < season;
        eprintln!(
            "{name} {} {}/{}: window {:.1} ms, ready {:.1} ms",
            if seasoning { "season" } else { "round" },
            if seasoning { at + 1 } else { at - season + 1 },
            if seasoning { season } else { rounds },
            round.window_ms,
            round.ready_ms
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
    for (metric, samples) in [
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
    ] {
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
    terminal: &ttybench_core::config::Terminal,
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
    })
}
