//! `ttybench-frames`: each frame of a terminal on the GPU. Each round
//! launches the terminal running the payload, records Instruments' Metal
//! System Trace of it, of `xctrace`, Xcode's, over `seconds` of a scroll as
//! `ttybench-scroll`'s, or of rest; then reads of the trace each of the
//! terminal's frames' GPU time, the frames a second, the gaps between them,
//! the frames that the display showed a second and the share of its
//! refreshes that showed a new one, as the display's own record tells it;
//! and removes the trace.
//!
//! ```text
//! ttybench-frames --terminal <name> [--terminals <file>] [--feature <name>]
//!                 [--workload scroll|rest] [--seconds <n>] [--by pixels|lines]
//!                 [--awake on|off] [--keep] [--rounds <n>]
//!                 [--pause <seconds>] [--session <folder>]
//! ```
//!
//! A trace takes some tens of megabytes a second: the tool looks for 2 GiB
//! free first, and removes the trace once read, whatever comes, unless
//! `--keep` keeps it for Instruments to open. The scroll
//! takes the Accessibility permission of the app that runs the tool; leave
//! the mouse and the trackpad alone meanwhile.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, SystemTime};

use ttybench_core::awake::Awake;
use ttybench_core::cli::{self, Args};
use ttybench_core::config::Terminal;
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Machine, Record, Session};
use ttybench_core::scroll::{self as pattern, By, Pattern};
use ttybench_core::xctrace::{self, Recording, Table, Target};
use ttybench_core::{stats, terminal};

/// How long a round waits for the terminal's window, its payload and its
/// lines.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long a terminal takes to quit, at most.
const QUIT: Duration = Duration::from_secs(5);

/// How long the window rests once it shows, before the measure.
const SETTLE: Duration = Duration::from_secs(2);

/// The free bytes that a recording asks for, at least.
const FREE: u64 = 2 << 30;

/// The template recorded.
const TEMPLATE: &str = "Metal System Trace";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-frames: {why}");
            ExitCode::FAILURE
        }
    }
}

/// What the terminal does while it is recorded.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Workload {
    /// It rests, its window still.
    Rest,
    /// Its window scrolls, of this pattern.
    Scroll(Pattern),
}

/// A round's settings.
#[derive(Clone, Copy, Debug)]
struct Settings {
    workload: Workload,
    seconds: f64,
    awake: bool,
    keep: bool,
}

/// A round's figures: each frame's GPU time and the gaps between the
/// frames, in milliseconds; the frames a second; and the share of the
/// refreshes that showed a new frame, if the display told its refreshes.
#[derive(Clone, Debug, PartialEq)]
struct Figures {
    gpu_ms: Vec<f64>,
    gaps_ms: Vec<f64>,
    frames_per_s: f64,
    shown: Option<Shown>,
}

fn run() -> Result<(), String> {
    let args = Args::parse(
        std::env::args().skip(1),
        &[
            "terminal",
            "terminals",
            "feature",
            "workload",
            "seconds",
            "by",
            "awake",
            "rounds",
            "pause",
            "session",
        ],
        &["keep"],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let seconds: f64 = args.number("seconds", 10.0)?;
    let workload = match args.value("workload").unwrap_or("scroll") {
        "rest" => Workload::Rest,
        "scroll" => {
            let by = match args.value("by").unwrap_or("pixels") {
                "pixels" => By::Pixels,
                "lines" => By::Lines,
                other => return Err(format!("--by takes pixels or lines, not `{other}`")),
            };
            ttybench_core::keys::may_post()?;
            Workload::Scroll(Pattern {
                by,
                seconds,
                rate: by.rate(),
                amplitude: 200.0,
            })
        }
        other => return Err(format!("--workload takes scroll or rest, not `{other}`")),
    };
    if seconds < pattern::PERIOD {
        return Err(format!("--seconds takes {} at least", pattern::PERIOD));
    }
    let settings = Settings {
        workload,
        seconds,
        awake: match args.value("awake").unwrap_or("on") {
            "on" => true,
            "off" => false,
            other => return Err(format!("--awake takes on or off, not `{other}`")),
        },
        keep: args.flag("keep"),
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
        let figures = round(&name, &terminal, &binary, &home, settings)?;
        let gpu = stats::summary(&figures.gpu_ms);
        eprintln!(
            "{name} round {}/{rounds}: {:.1} frames/s, a frame's GPU time {:.3} ms at the \
             median and {:.3} at the 95th percentile{}",
            at + 1,
            figures.frames_per_s,
            gpu.map_or(f64::NAN, |gpu| gpu.median),
            gpu.map_or(f64::NAN, |gpu| gpu.p95),
            figures.shown.map_or(String::new(), |shown| format!(
                ", {:.1} shown a second, {:.1} % of the refreshes of a new frame, {} frames \
                 dropped",
                shown.per_s,
                shown.share * 100.0,
                shown.dropped
            )),
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
    conditions.insert("seconds".into(), settings.seconds.into());
    match settings.workload {
        Workload::Rest => {
            conditions.insert("workload".into(), "rest".into());
        }
        Workload::Scroll(pattern) => {
            conditions.insert("workload".into(), "scroll".into());
            conditions.insert("by".into(), pattern.by.name().into());
        }
    }
    conditions.insert("awake".into(), settings.awake.into());
    if let Some(refresh) = Machine::this().refresh_hz {
        conditions.insert("refresh_hz".into(), refresh.into());
    }
    let metrics: Vec<(&str, &str, Vec<f64>)> = vec![
        (
            "frame_gpu_ms",
            "ms",
            measured.iter().flat_map(|f| f.gpu_ms.clone()).collect(),
        ),
        (
            "frames_per_s",
            "frames/s",
            measured.iter().map(|f| f.frames_per_s).collect(),
        ),
        (
            "frame_gap_ms",
            "ms",
            measured.iter().flat_map(|f| f.gaps_ms.clone()).collect(),
        ),
        (
            "refreshes_with_frame",
            "%",
            measured
                .iter()
                .filter_map(|f| f.shown.map(|shown| shown.share * 100.0))
                .collect(),
        ),
        (
            "frames_shown_per_s",
            "frames/s",
            measured
                .iter()
                .filter_map(|f| f.shown.map(|shown| shown.per_s))
                .collect(),
        ),
        (
            "frames_dropped",
            "frames",
            measured
                .iter()
                .filter_map(|f| f.shown.map(|shown| shown.dropped as f64))
                .collect(),
        ),
    ];
    for (metric, unit, samples) in metrics {
        if samples.is_empty() {
            continue;
        }
        if let Some(summary) = stats::summary(&samples) {
            println!(
                "{name} {metric}: median {:.3} {unit}, p95 {:.3}, of {} samples",
                summary.median, summary.p95, summary.count
            );
        }
        session
            .append(&Record {
                tool: "frames".into(),
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

/// A round: the terminal launched, recorded over its workload, then quit,
/// its trace read and removed.
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
    let workload: &[&str] = match settings.workload {
        Workload::Rest => &["rest"],
        Workload::Scroll(_) => &pattern::PAYLOAD,
    };
    let payload =
        Payload::new(binary, workload).map_err(|error| format!("the payload: {error}"))?;
    let (launched, _) = terminal::launch_ready(name, terminal, &payload, home, TIMEOUT)?;
    let path = folder.join(format!("ttybench-frames-{}.trace", std::process::id()));
    let recorded = match settings.workload {
        Workload::Rest => Ok(()),
        Workload::Scroll(_) => payload
            .wait("done", TIMEOUT)
            .map(|_| ())
            .ok_or_else(|| format!("{name} printed its lines in no {} s", TIMEOUT.as_secs())),
    }
    .and_then(|()| record(name, &launched, settings, &path));
    let tree = launched.tree();
    launched
        .quit(QUIT)
        .map_err(|error| format!("{name} does not quit: {error}"))?;
    let (trace, start, end) = recorded?;
    let figures = figures(name, trace.path(), &tree, start, end);
    if settings.keep {
        eprintln!("the trace kept: {}", trace.keep().display());
    }
    figures
}

/// The terminal's workload recorded into `path`: the trace, and the
/// workload's span by the wall clock.
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
    let (x, y) = (
        window.frame.x + window.frame.width / 2.0,
        window.frame.y + window.frame.height / 2.0,
    );
    if matches!(settings.workload, Workload::Scroll(_)) {
        pattern::away(x, y);
    }
    let limit = Duration::from_secs_f64(settings.seconds + 3.0);
    let recording = Recording::start(TEMPLATE, Target::Process(window.pid), limit, path)?;
    thread::sleep(Duration::from_millis(500));
    let start = SystemTime::now();
    match settings.workload {
        Workload::Rest => thread::sleep(Duration::from_secs_f64(settings.seconds)),
        Workload::Scroll(scroll) => {
            let events = pattern::plan(scroll);
            pattern::post(x, y, &events, scroll.tick());
        }
    }
    let end = SystemTime::now();
    let trace = recording.finish()?;
    Ok((trace, start, end))
}

/// The figures of the trace at `trace` of the processes `tree`, over the
/// workload from `start` to `end` by the wall clock.
fn figures(
    name: &str,
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
    let span = (offset(start), offset(end));
    let intervals = xctrace::table(trace, "metal-gpu-intervals")?;
    let frames = frames_of(&intervals, tree, span);
    if frames.is_empty() {
        return Err(format!("{name} drew no frame on the GPU in the trace"));
    }
    let seconds = span.1.saturating_sub(span.0) as f64 / 1e9;
    let starts: Vec<u64> = frames.values().map(|frame| frame.0).collect();
    let mut sorted = starts.clone();
    sorted.sort_unstable();
    // The display's surfaces tell which frames it showed; its vsyncs'
    // table holds requests of vsyncs, which an app's timer multiplies.
    let shown = xctrace::table(trace, "displayed-surfaces-interval")
        .ok()
        .and_then(|surfaces| {
            let refresh = refresh_of(&surfaces)?;
            tree.iter()
                .filter_map(|&pid| shown_of(&surfaces, pid, span, refresh))
                .max_by(|a, b| a.share.total_cmp(&b.share))
        });
    Ok(Figures {
        gpu_ms: frames.values().map(|frame| frame.1 as f64 / 1e6).collect(),
        gaps_ms: sorted
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) as f64 / 1e6)
            .collect(),
        frames_per_s: frames.len() as f64 / seconds,
        shown,
    })
}

/// The frames of the processes `pids` that started within `span`, of the
/// GPU's intervals: each frame's first start and its GPU time, the union of
/// its intervals, as its vertex and fragment work may overlap; by each
/// process's frame number.
fn frames_of(
    intervals: &Table,
    pids: &[libc::pid_t],
    span: (u64, u64),
) -> BTreeMap<(libc::pid_t, u64), (u64, u64)> {
    let (Some(start), Some(duration), Some(frame), Some(process)) = (
        intervals.column("start"),
        intervals.column("duration"),
        intervals.column("frame-number"),
        intervals.column("process"),
    ) else {
        return BTreeMap::new();
    };
    let mut spans: BTreeMap<(libc::pid_t, u64), Vec<(u64, u64)>> = BTreeMap::new();
    for row in &intervals.rows {
        let (Some(pid), Some(number), Some(at), Some(length)) = (
            row[process].pid(),
            row[frame].number(),
            row[start].number(),
            row[duration].number(),
        ) else {
            continue;
        };
        if pids.contains(&pid) {
            spans
                .entry((pid, number))
                .or_default()
                .push((at, at + length));
        }
    }
    spans
        .into_iter()
        .filter_map(|(key, mut parts)| {
            parts.sort_unstable();
            let first = parts.first()?.0;
            if first < span.0 || first > span.1 {
                return None;
            }
            // The union of the parts.
            let (mut busy, mut reach) = (0, 0);
            for (from, to) in parts {
                let from = from.max(reach);
                if to > from {
                    busy += to - from;
                }
                reach = reach.max(to);
            }
            Some((key, (first, busy)))
        })
        .collect()
}

/// The frame of the process `pid` that a displayed surface's label names,
/// as `Surface 389 (ghostty (78543):Frame 1:Surface 439)`, its number of
/// the thousands' separators that the label writes, as `Frame 1'000`.
fn frame_shown(label: &str, pid: libc::pid_t) -> Option<u64> {
    let mark = format!("({pid}):Frame ");
    let at = label.find(&mark)? + mark.len();
    let digits: String = label[at..]
        .chars()
        .take_while(|c| {
            c.is_ascii_digit() || matches!(c, '\'' | ',' | '.' | '\u{2019}' | '\u{a0}' | '\u{202f}')
        })
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The display's showing of the frames of the process `pid` within
/// `span`, of refreshes `refresh` long, as the display's surfaces tell it:
/// the share of the refreshes that showed a new frame, the frames shown
/// over the refreshes, each swap's surface naming the frame that it shows;
/// and the frames dropped, drawn but never shown, the numbers that the
/// frames shown pass over. A swap's time strays from its refresh's, two
/// swaps a refresh apart may be written less than half of one apart: the
/// frames count, not the times. None of no frame of the process shown.
fn shown_of(surfaces: &Table, pid: libc::pid_t, span: (u64, u64), refresh: u64) -> Option<Shown> {
    let (start, label) = (surfaces.column("start")?, surfaces.column("event-label")?);
    let refreshes = span.1.saturating_sub(span.0) as f64 / refresh.max(1) as f64;
    if refreshes < 1.0 {
        return None;
    }
    let mut frames: Vec<u64> = Vec::new();
    for row in &surfaces.rows {
        let Some(at) = row[start].number() else {
            continue;
        };
        if at < span.0 || at >= span.1 {
            continue;
        }
        let Some(frame) = row[label]
            .fmt
            .as_deref()
            .and_then(|label| frame_shown(label, pid))
        else {
            continue;
        };
        if frames.last() != Some(&frame) {
            frames.push(frame);
        }
    }
    if frames.is_empty() {
        return None;
    }
    let dropped = frames
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]).saturating_sub(1))
        .sum();
    Some(Shown {
        share: (frames.len() as f64 / refreshes).min(1.0),
        per_s: frames.len() as f64 / (span.1.saturating_sub(span.0) as f64 / 1e9),
        dropped,
    })
}

/// How the display showed a process's frames over a span.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shown {
    /// The share of the refreshes that showed a new frame.
    share: f64,
    /// The frames shown a second, which a display of a variable refresh
    /// tells better than a share of a rate that it changes.
    per_s: f64,
    /// The frames drawn but never shown.
    dropped: u64,
}

/// The refresh of the display that the surfaces `surfaces` swapped on, in
/// nanoseconds: of its rate where it tells one, as a display of a fixed
/// refresh does; else the median time a surface stayed on screen.
fn refresh_of(surfaces: &Table) -> Option<u64> {
    if let Some(hz) = Machine::this().refresh_hz.filter(|hz| *hz > 0.0) {
        return Some((1e9 / hz) as u64);
    }
    let duration = surfaces.column("duration")?;
    let mut lengths: Vec<u64> = surfaces
        .rows
        .iter()
        .filter_map(|row| row[duration].number())
        .filter(|&length| length > 0)
        .collect();
    lengths.sort_unstable();
    lengths.get(lengths.len() / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttybench_core::xctrace::Cell;

    fn cell(text: &str) -> Cell {
        Cell {
            text: Some(text.into()),
            fmt: Some(text.into()),
        }
    }

    fn named(fmt: &str) -> Cell {
        Cell {
            text: None,
            fmt: Some(fmt.into()),
        }
    }

    #[test]
    fn a_frames_gpu_time_is_the_union_of_its_intervals() {
        let intervals = Table {
            columns: ["start", "duration", "frame-number", "process"]
                .map(String::from)
                .to_vec(),
            rows: vec![
                // Frame 1: vertex 100..120, fragment 110..200: 100 busy.
                vec![cell("100"), cell("20"), cell("1"), named("ghostty (7)")],
                vec![cell("110"), cell("90"), cell("1"), named("ghostty (7)")],
                // Frame 2 at 300, 50 long.
                vec![cell("300"), cell("50"), cell("2"), named("ghostty (7)")],
                // Another process's, and a frame before the span.
                vec![
                    cell("120"),
                    cell("500"),
                    cell("1"),
                    named("Code Helper (9)"),
                ],
                vec![cell("10"), cell("5"), cell("0"), named("ghostty (7)")],
            ],
        };
        let frames = frames_of(&intervals, &[7], (50, 1000));
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[&(7, 1)], (100, 100));
        assert_eq!(frames[&(7, 2)], (300, 50));
    }

    #[test]
    fn the_share_counts_the_refreshes_of_a_new_frame() {
        let label = |frame: u64| named(&format!("Surface 3 (ghostty (7):Frame {frame}:Surface 9)"));
        // Swaps of 10 a refresh, one of two refreshes, over 100: ten
        // refreshes, four of a new frame.
        let surfaces = Table {
            columns: vec!["start".into(), "duration".into(), "event-label".into()],
            rows: vec![
                vec![cell("0"), cell("10"), label(1)],
                vec![cell("10"), cell("10"), label(2)],
                vec![cell("20"), cell("10"), label(2)],
                vec![cell("30"), cell("10"), label(3)],
                vec![cell("40"), cell("10"), named("Surface 196")],
                vec![cell("50"), cell("20"), label(4)],
                vec![cell("70"), cell("10"), named("Surface 196")],
                vec![cell("80"), cell("10"), named("Surface 196")],
                vec![cell("90"), cell("10"), named("Surface 196")],
            ],
        };
        let share = shown_of(&surfaces, 7, (0, 100), 10).unwrap().share;
        assert!((share - 0.4).abs() < 1e-9, "{share}");
        assert_eq!(shown_of(&surfaces, 8, (0, 100), 10), None);
        // Swaps a little off their refreshes count each its nearest.
        let strayed = Table {
            columns: vec!["start".into(), "duration".into(), "event-label".into()],
            rows: vec![
                vec![cell("1"), cell("8"), label(1)],
                vec![cell("9"), cell("12"), label(2)],
                vec![cell("21"), cell("8"), label(3)],
                vec![cell("29"), cell("12"), label(4)],
            ],
        };
        let share = shown_of(&strayed, 7, (0, 40), 10).unwrap().share;
        assert!((share - 1.0).abs() < 1e-9, "{share}");
        // Swaps of every refresh about its half, from the span's start,
        // straying either way: each its own refresh still.
        let halves = Table {
            columns: vec!["start".into(), "duration".into(), "event-label".into()],
            rows: [4, 16, 24, 36, 44, 56, 64, 76]
                .iter()
                .enumerate()
                .map(|(at, start)| vec![cell(&start.to_string()), cell("10"), label(at as u64 + 1)])
                .collect(),
        };
        let share = shown_of(&halves, 7, (0, 80), 10).unwrap().share;
        assert!((share - 1.0).abs() < 1e-9, "{share}");
        // Swaps written within half a refresh of each other, as the
        // display's record writes some: each frame shown counts; frames
        // passed over count dropped.
        let bunched = Table {
            columns: vec!["start".into(), "duration".into(), "event-label".into()],
            rows: vec![
                vec![cell("1"), cell("10"), label(1)],
                vec![cell("3"), cell("10"), label(2)],
                vec![cell("21"), cell("10"), label(3)],
                vec![cell("30"), cell("10"), label(4)],
                vec![cell("41"), cell("10"), label(7)],
            ],
        };
        let shown = shown_of(&bunched, 7, (0, 40), 10).unwrap();
        assert!((shown.share - 1.0).abs() < 1e-9, "{shown:?}");
        assert_eq!(shown.dropped, 0);
        let shown = shown_of(&bunched, 7, (0, 50), 10).unwrap();
        assert!((shown.share - 1.0).abs() < 1e-9, "{shown:?}");
        assert_eq!(shown.dropped, 2);
        assert_eq!(
            frame_shown(
                "Surface 3 (Code Helper (9):Frame 2:Surface 1/kitty (12):Frame 40:Surface 9)",
                12
            ),
            Some(40)
        );
        assert_eq!(
            frame_shown(
                "Surface 17 (lokyr (82502):Frame 1'227:Surface 517 cannot go direct)",
                82502
            ),
            Some(1227)
        );
        assert_eq!(
            frame_shown("Surface 17 (lokyr (5):Frame 12,345:Surface 9)", 5),
            Some(12345)
        );
    }
}
