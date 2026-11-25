//! `ttybench-motion`: how smoothly a terminal's window moves. Each round
//! launches the terminal running the payload at rest, records Instruments'
//! Animation Hitches of every process, of `xctrace`, Xcode's, while the
//! mouse drags the window by its titlebar or resizes it by its bottom-right
//! corner, along a sine as a hand moves back and forth; then reads of the
//! trace the frames that the display showed a second, those that it
//! missed, the compositor's delay of each, and the CPU of the terminal and
//! of WindowServer, over the motion less its first and its last 0.3 s; and
//! removes the trace.
//!
//! ```text
//! ttybench-motion --terminal <name> [--terminals <file>] [--feature <name>]
//!                 [--kind drag|resize] [--seconds <n>] [--rate <n>]
//!                 [--amplitude <points>] [--awake on|off] [--keep]
//!                 [--rounds <n>] [--pause <seconds>] [--session <folder>]
//! ```
//!
//! The mouse's events go through the window server, of the Accessibility
//! permission of the app that runs the tool: leave the mouse and the
//! trackpad alone meanwhile. A trace of every process takes much room:
//! the tool asks for 4 GiB free, and removes the trace once read, unless
//! `--keep` keeps it.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use ttybench::awake::Awake;
use ttybench::cli::{self, Args};
use ttybench::config::Terminal;
use ttybench::keys::{self, Button};
use ttybench::payload::{self, Payload};
use ttybench::results::{self, Machine, Record, Session};
use ttybench::xctrace::{self, Recording, Table, Target};
use ttybench::{stats, terminal};

/// How long a round waits for the terminal's window and its payload.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long the terminal's window takes to come to the front where the
/// pointer's events go, at most.
const FRONT: Duration = Duration::from_secs(3);

/// How long a terminal takes to quit, at most.
const QUIT: Duration = Duration::from_secs(5);

/// How long the window rests once it shows, before the measure.
const SETTLE: Duration = Duration::from_secs(2);

/// The free bytes that a recording of every process asks for, at least.
const FREE: u64 = 4 << 30;

/// The template recorded.
const TEMPLATE: &str = "Animation Hitches";

/// The period of the sine, as a hand moves back and forth, in seconds.
const PERIOD: f64 = 2.0;

/// The motion's start and end left out, as the window starts and stops.
const TRIM: u64 = 300_000_000;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-motion: {why}");
            ExitCode::FAILURE
        }
    }
}

/// What the mouse moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The window, by its titlebar.
    Drag,
    /// The window's size, by its bottom-right corner.
    Resize,
}

/// A round's settings.
#[derive(Clone, Copy, Debug)]
struct Settings {
    kind: Kind,
    seconds: f64,
    rate: f64,
    amplitude: f64,
    awake: bool,
    keep: bool,
}

/// A round's figures: the frames that the display showed a second, those
/// that it missed, its refresh in milliseconds, the compositor's delay of
/// each frame in milliseconds, and the CPU of the terminal and of
/// WindowServer in percent of a core.
#[derive(Clone, Debug, PartialEq)]
struct Figures {
    frames_per_s: f64,
    missed: u64,
    refresh_ms: f64,
    delays_ms: Vec<f64>,
    terminal_cpu: f64,
    window_server_cpu: f64,
}

fn run() -> Result<(), String> {
    let args = Args::parse(
        std::env::args().skip(1),
        &[
            "terminal",
            "terminals",
            "feature",
            "kind",
            "seconds",
            "rate",
            "amplitude",
            "awake",
            "rounds",
            "pause",
            "session",
        ],
        &["keep"],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    keys::may_post()?;
    let settings = Settings {
        kind: match args.value("kind").unwrap_or("drag") {
            "drag" => Kind::Drag,
            "resize" => Kind::Resize,
            other => return Err(format!("--kind takes drag or resize, not `{other}`")),
        },
        seconds: args.number("seconds", 10.0)?,
        rate: args.number("rate", 120.0)?,
        amplitude: args.number("amplitude", 200.0)?,
        awake: match args.value("awake").unwrap_or("on") {
            "on" => true,
            "off" => false,
            other => return Err(format!("--awake takes on or off, not `{other}`")),
        },
        keep: args.flag("keep"),
    };
    if settings.seconds < 1.0 || settings.rate < 10.0 {
        return Err("--seconds takes 1 at least, and --rate 10 at least".into());
    }
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
        let figures = round(&name, &terminal, &binary, &home, settings)?;
        let delays = stats::summary(&figures.delays_ms);
        eprintln!(
            "{name} round {}/{rounds}: {:.1} frames/s of a refresh of {:.2} ms, {} missed, the \
             compositor's delay {:.2} ms at the median and {:.2} at the 95th percentile; the \
             terminal's CPU {:.1} %, WindowServer's {:.1} %",
            at + 1,
            figures.frames_per_s,
            figures.refresh_ms,
            figures.missed,
            delays.map_or(f64::NAN, |delays| delays.median),
            delays.map_or(f64::NAN, |delays| delays.p95),
            figures.terminal_cpu,
            figures.window_server_cpu,
        );
        measured.push(figures);
        if at + 1 < rounds {
            thread::sleep(pause);
        }
    }
    let version = results::app_version(&terminal.app);
    let mut conditions = serde_json::Map::new();
    if let Some(feature) = cli::feature(&args) {
        conditions.insert("feature".into(), feature.into());
    }
    conditions.insert("rounds".into(), rounds.into());
    conditions.insert(
        "kind".into(),
        match settings.kind {
            Kind::Drag => "drag",
            Kind::Resize => "resize",
        }
        .into(),
    );
    conditions.insert("seconds".into(), settings.seconds.into());
    conditions.insert("rate".into(), settings.rate.into());
    conditions.insert("amplitude".into(), settings.amplitude.into());
    conditions.insert("awake".into(), settings.awake.into());
    if let Some(refresh) = Machine::this().refresh_hz {
        conditions.insert("refresh_hz".into(), refresh.into());
    }
    let metrics: Vec<(&str, &str, Vec<f64>)> = vec![
        (
            "frames_per_s",
            "frames/s",
            measured.iter().map(|f| f.frames_per_s).collect(),
        ),
        (
            "missed_frames",
            "frames",
            measured.iter().map(|f| f.missed as f64).collect(),
        ),
        (
            "compositor_delay_ms",
            "ms",
            measured.iter().flat_map(|f| f.delays_ms.clone()).collect(),
        ),
        (
            "terminal_cpu_percent",
            "%",
            measured.iter().map(|f| f.terminal_cpu).collect(),
        ),
        (
            "window_server_cpu_percent",
            "%",
            measured.iter().map(|f| f.window_server_cpu).collect(),
        ),
    ];
    for (metric, unit, samples) in metrics {
        if let Some(summary) = stats::summary(&samples) {
            println!(
                "{name} {metric}: median {:.2} {unit}, p95 {:.2}, of {} samples",
                summary.median, summary.p95, summary.count
            );
        }
        session
            .append(&Record {
                tool: "motion".into(),
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

/// A round: the terminal launched, recorded as its window moves, then
/// quit, its trace read and removed.
fn round(
    name: &str,
    terminal: &Terminal,
    binary: &std::path::Path,
    home: &std::path::Path,
    settings: Settings,
) -> Result<Figures, String> {
    let folder = std::env::temp_dir();
    let free = xctrace::free_bytes(&folder).map_err(|error| format!("the free space: {error}"))?;
    if free < FREE {
        return Err(format!(
            "{} holds {} MiB free, of {} at least for a trace",
            folder.display(),
            free >> 20,
            FREE >> 20
        ));
    }
    let _awake = settings.awake.then(Awake::start);
    let payload =
        Payload::new(binary, &["rest"]).map_err(|error| format!("the payload: {error}"))?;
    let (launched, _) = terminal::launch_ready(name, terminal, &payload, home, TIMEOUT)?;
    let path = folder.join(format!("ttybench-motion-{}.trace", std::process::id()));
    let recorded = record(name, &launched, settings, &path);
    let tree = launched.tree();
    launched
        .quit(QUIT)
        .map_err(|error| format!("{name} does not quit: {error}"))?;
    let (trace, start, end) = recorded?;
    let figures = figures(trace.path(), &tree, start, end);
    if settings.keep {
        eprintln!("the trace kept: {}", trace.keep().display());
    }
    figures
}

/// The window moved while recorded into `path`: the trace, and the
/// motion's span by the wall clock.
fn record(
    name: &str,
    launched: &terminal::Launched,
    settings: Settings,
    path: &std::path::Path,
) -> Result<(xctrace::Trace, SystemTime, SystemTime), String> {
    launched
        .wait_window(TIMEOUT)
        .ok_or_else(|| format!("{name} showed no window"))?;
    thread::sleep(SETTLE);
    let window = launched
        .window()
        .ok_or_else(|| format!("{name}'s window went"))?;
    let frame = window.frame;
    // Within the titlebar, past its buttons; or just inside the corner.
    let (x, y) = match settings.kind {
        Kind::Drag => (frame.x + frame.width / 2.0, frame.y + 12.0),
        Kind::Resize => (frame.x + frame.width - 2.0, frame.y + frame.height - 2.0),
    };
    launched.front_at(x, y, FRONT)?;
    let limit = Duration::from_secs_f64(settings.seconds + 3.0);
    let recording = Recording::start(TEMPLATE, Target::All, limit, path)?;
    thread::sleep(Duration::from_millis(500));
    keys::point(x, y);
    thread::sleep(Duration::from_millis(100));
    keys::button(Button::Down, x, y);
    thread::sleep(Duration::from_millis(100));
    let start = SystemTime::now();
    let steps = (settings.seconds * settings.rate) as usize;
    let tick = Duration::from_secs_f64(1.0 / settings.rate);
    let origin = Instant::now();
    let mut at = (x, y);
    for step in 1..=steps {
        let offset = offset(settings, step);
        at = match settings.kind {
            Kind::Drag => (x + offset, y + offset / 4.0),
            Kind::Resize => (x + offset / 2.0, y + offset / 4.0),
        };
        keys::button(Button::Dragged, at.0, at.1);
        let due = origin + tick.mul_f64(step as f64);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
    }
    keys::button(Button::Up, at.0, at.1);
    let end = SystemTime::now();
    let trace = recording.finish()?;
    Ok((trace, start, end))
}

/// The sine's offset at the `step`-th event, in points.
fn offset(settings: Settings, step: usize) -> f64 {
    let time = step as f64 / settings.rate;
    settings.amplitude * (std::f64::consts::TAU * time / PERIOD).sin()
}

/// The figures of the trace at `trace`, of the processes `tree`, over the
/// motion from `start` to `end` by the wall clock less its first and its
/// last 0.3 s.
fn figures(
    trace: &std::path::Path,
    tree: &[libc::pid_t],
    start: SystemTime,
    end: SystemTime,
) -> Result<Figures, String> {
    let began = xctrace::start(trace)?;
    let offset = |at: SystemTime| {
        at.duration_since(began).map_or(0, |since| {
            u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
        })
    };
    let span = (offset(start) + TRIM, offset(end).saturating_sub(TRIM));
    if span.1 <= span.0 {
        return Err("the motion was too short to measure".into());
    }
    let swaps = xctrace::table(trace, "display-surface-swap")?;
    let (shown, delays) =
        swaps_of(&swaps, span).ok_or("the display showed no frame of the motion")?;
    let seconds = (span.1 - span.0) as f64 / 1e9;
    let gaps: Vec<u64> = shown.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let mut sorted = gaps.clone();
    sorted.sort_unstable();
    let period = sorted.get(sorted.len() / 2).copied().unwrap_or(0).max(1);
    let missed = gaps
        .iter()
        .map(|&gap| ((gap as f64 / period as f64).round() as u64).saturating_sub(1))
        .sum();
    let usage = xctrace::table(trace, "ProcessCPUUsage").ok();
    let cpu = |matches: &dyn Fn(&str, Option<libc::pid_t>) -> bool| {
        usage
            .as_ref()
            .map_or(0.0, |usage| cpu_of(usage, span, matches) / seconds)
    };
    Ok(Figures {
        frames_per_s: shown.len() as f64 / seconds,
        missed,
        refresh_ms: period as f64 / 1e6,
        delays_ms: delays,
        terminal_cpu: cpu(&|_, pid| pid.is_some_and(|pid| tree.contains(&pid))),
        window_server_cpu: cpu(&|name, _| name.starts_with("WindowServer (")),
    })
}

/// The swaps within `span` of the display that showed the most frames:
/// their times, and the compositor's delay of each, in milliseconds.
fn swaps_of(swaps: &Table, span: (u64, u64)) -> Option<(Vec<u64>, Vec<f64>)> {
    let (time, delay, display) = (
        swaps.column("timestamp")?,
        swaps.column("delay")?,
        swaps.column("display-name")?,
    );
    let mut by_display: BTreeMap<String, (Vec<u64>, Vec<f64>)> = BTreeMap::new();
    for row in &swaps.rows {
        let Some(at) = row[time].number() else {
            continue;
        };
        if at < span.0 || at > span.1 {
            continue;
        }
        let name = row[display]
            .fmt
            .clone()
            .or_else(|| row[display].text.clone())
            .unwrap_or_default();
        let entry = by_display.entry(name).or_default();
        entry.0.push(at);
        entry.1.push(row[delay].number().unwrap_or(0) as f64 / 1e6);
    }
    by_display
        .into_values()
        .max_by_key(|(shown, _)| shown.len())
        .filter(|(shown, _)| !shown.is_empty())
}

/// The CPU within `span` of the processes that `matches` takes, by their
/// name and process, of the table of processes' CPU usage, whose usage is
/// in percent of a core: in percent of a core times seconds.
fn cpu_of(
    usage: &Table,
    span: (u64, u64),
    matches: &dyn Fn(&str, Option<libc::pid_t>) -> bool,
) -> f64 {
    let (Some(start), Some(duration), Some(cpu), Some(process)) = (
        usage.column("start"),
        usage.column("duration"),
        usage.column("cpu-usage"),
        usage.column("process"),
    ) else {
        return 0.0;
    };
    usage
        .rows
        .iter()
        .filter(|row| {
            row[start]
                .number()
                .is_some_and(|at| at >= span.0 && at <= span.1)
        })
        .filter(|row| {
            let name = row[process].fmt.as_deref().unwrap_or("");
            matches(name, row[process].pid())
        })
        .map(|row| {
            let share: f64 = row[cpu]
                .text
                .as_deref()
                .and_then(|text| text.parse().ok())
                .unwrap_or(0.0);
            share * row[duration].number().unwrap_or(0) as f64 / 1e9
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttybench::xctrace::Cell;

    fn cell(text: &str) -> Cell {
        Cell {
            text: Some(text.into()),
            fmt: Some(text.into()),
        }
    }

    #[test]
    fn the_busiest_displays_swaps_within_the_span_count() {
        let swaps = Table {
            columns: ["timestamp", "delay", "display-name"]
                .map(String::from)
                .to_vec(),
            rows: vec![
                vec![cell("100"), cell("2000000"), cell("Display 1")],
                vec![cell("200"), cell("3000000"), cell("Display 1")],
                vec![cell("300"), cell("1000000"), cell("Display 1")],
                vec![cell("150"), cell("9000000"), cell("Display 2")],
                vec![cell("900"), cell("1000000"), cell("Display 1")],
            ],
        };
        let (shown, delays) = swaps_of(&swaps, (50, 500)).unwrap();
        assert_eq!(shown, [100, 200, 300]);
        assert_eq!(delays, [2.0, 3.0, 1.0]);
        assert_eq!(swaps_of(&swaps, (1000, 2000)), None);
    }

    #[test]
    fn the_cpu_of_a_process_sums_its_rows_within_the_span() {
        let named = |fmt: &str| Cell {
            text: None,
            fmt: Some(fmt.into()),
        };
        let usage = Table {
            columns: ["start", "duration", "cpu-usage", "process"]
                .map(String::from)
                .to_vec(),
            rows: vec![
                vec![
                    cell("100"),
                    cell("1000000000"),
                    cell("0.5"),
                    named("WindowServer (429)"),
                ],
                vec![
                    cell("200"),
                    cell("1000000000"),
                    cell("0.25"),
                    named("lokyr (77)"),
                ],
                vec![
                    cell("300"),
                    cell("1000000000"),
                    cell("1.0"),
                    named("lokyr (77)"),
                ],
                vec![
                    cell("9000"),
                    cell("1000000000"),
                    cell("1.0"),
                    named("lokyr (77)"),
                ],
            ],
        };
        let lokyr = cpu_of(&usage, (0, 1000), &|_, pid| pid == Some(77));
        assert!((lokyr - 1.25).abs() < 1e-9, "{lokyr}");
        let server = cpu_of(&usage, (0, 1000), &|name, _| {
            name.starts_with("WindowServer (")
        });
        assert!((server - 0.5).abs() < 1e-9, "{server}");
    }

    #[test]
    fn the_sine_goes_out_and_back_each_period() {
        let settings = Settings {
            kind: Kind::Drag,
            seconds: 2.0,
            rate: 120.0,
            amplitude: 200.0,
            awake: false,
            keep: false,
        };
        assert!(offset(settings, 0).abs() < 1e-9);
        assert!((offset(settings, 60) - 200.0).abs() < 1e-9);
        assert!(offset(settings, 240).abs() < 1e-6);
    }
}
