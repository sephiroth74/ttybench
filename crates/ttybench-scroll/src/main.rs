//! `ttybench-scroll`: how smoothly a terminal scrolls. Each round launches
//! the terminal running the payload, which prints lines enough for a long
//! scrollback; captures the terminal's window; scrolls it, as a trackpad
//! does, of precise pixels, its fingers' swipes and their momentum, or as a
//! mouse wheel does, of lines; and counts the frames that the window server
//! took of the text moved: the frames a second, and the gaps between them.
//!
//! ```text
//! ttybench-scroll --terminal <name> [--terminals <file>] [--feature <name>] [--by pixels|lines]
//!                 [--seconds <n>] [--rate <n>] [--amplitude <points>]
//!                 [--source window|screen] [--awake on|off]
//!                 [--rounds <n>] [--pause <seconds>] [--session <folder>]
//! ```
//!
//! By pixels, each period of 2 seconds swipes up `amplitude` points over a
//! quarter of it, `rate` events a second, then lets its momentum slow over
//! the next quarter to a pixel an event, then the same down, so that the
//! view moves throughout and comes back where it was. By lines, each period
//! turns the wheel up a step at a time, `rate` steps a second, for half of
//! it, then down.
//!
//! The frames are those that the window server took of the window, as it
//! took them: the refreshes that showed them are the display's own record,
//! which a trace of the display tells, not the capture.
//!
//! The scroll goes through the window server to the window under the
//! pointer, which moves to the window's middle: leave the mouse and the
//! trackpad alone meanwhile. It takes the Accessibility permission, and the
//! capture the Screen Recording permission, of the app that runs the tool.

use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ttybench_core::awake::Awake;
use ttybench_core::capture::{Capture, Source};
use ttybench_core::cli::{self, Args};
use ttybench_core::config::Terminal;
use ttybench_core::frames::{self, Frame, Rect};
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Machine, Record, Session};
use ttybench_core::scroll::{self as pattern, By, Pattern};
use ttybench_core::{stats, terminal};

/// How long a round waits for the terminal's window, its payload and its
/// lines.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long the terminal's window takes to come to the front where the
/// pointer's events go, at most.
const FRONT: Duration = Duration::from_secs(3);

/// How long a terminal takes to quit, at most.
const QUIT: Duration = Duration::from_secs(5);

/// How long the window rests once its lines are printed, before its
/// capture.
const SETTLE: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-scroll: {why}");
            ExitCode::FAILURE
        }
    }
}

/// A round's settings.
#[derive(Clone, Copy, Debug)]
struct Settings {
    pattern: Pattern,
    source: Source,
    awake: bool,
}

/// A round's figures: the frames a second, and the gaps between them, in
/// milliseconds.
#[derive(Clone, Debug, PartialEq)]
struct Figures {
    frames_per_s: f64,
    gaps_ms: Vec<f64>,
}

fn run() -> Result<(), String> {
    let args = Args::parse(
        std::env::args().skip(1),
        &[
            "terminal",
            "terminals",
            "feature",
            "by",
            "seconds",
            "rate",
            "amplitude",
            "rounds",
            "pause",
            "session",
            "source",
            "awake",
        ],
        &[],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    ttybench_core::keys::may_post()?;
    let by = match args.value("by").unwrap_or("pixels") {
        "pixels" => By::Pixels,
        "lines" => By::Lines,
        other => return Err(format!("--by takes pixels or lines, not `{other}`")),
    };
    let settings = Settings {
        pattern: Pattern {
            by,
            seconds: args.number("seconds", 10.0)?,
            rate: args.number("rate", by.rate())?,
            amplitude: args.number("amplitude", 200.0)?,
        },
        source: match args.value("source").unwrap_or("window") {
            "screen" => Source::Screen,
            "window" => Source::Window,
            other => return Err(format!("--source takes screen or window, not `{other}`")),
        },
        awake: match args.value("awake").unwrap_or("on") {
            "on" => true,
            "off" => false,
            other => return Err(format!("--awake takes on or off, not `{other}`")),
        },
    };
    if settings.pattern.seconds < pattern::PERIOD || settings.pattern.rate < 4.0 {
        return Err(format!(
            "--seconds takes {} at least, and --rate 4 at least",
            pattern::PERIOD
        ));
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
        let gaps = stats::summary(&figures.gaps_ms);
        eprintln!(
            "{name} round {}/{rounds}: {:.1} frames/s, gaps of {:.1} ms at the median and \
             {:.1} at the 95th percentile",
            at + 1,
            figures.frames_per_s,
            gaps.map_or(f64::NAN, |gaps| gaps.median),
            gaps.map_or(f64::NAN, |gaps| gaps.p95),
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
    conditions.insert("by".into(), settings.pattern.by.name().into());
    conditions.insert("seconds".into(), settings.pattern.seconds.into());
    conditions.insert("awake".into(), settings.awake.into());
    conditions.insert(
        "source".into(),
        match settings.source {
            Source::Screen => "screen",
            Source::Window => "window",
        }
        .into(),
    );
    conditions.insert("rate".into(), settings.pattern.rate.into());
    if settings.pattern.by == By::Pixels {
        conditions.insert("amplitude".into(), settings.pattern.amplitude.into());
    }
    if let Some(refresh) = Machine::this().refresh_hz {
        conditions.insert("refresh_hz".into(), refresh.into());
    }
    let frames: Vec<f64> = measured.iter().map(|f| f.frames_per_s).collect();
    let gaps: Vec<f64> = measured
        .iter()
        .flat_map(|f| f.gaps_ms.iter().copied())
        .collect();
    if let (Some(frames), Some(gaps)) = (stats::summary(&frames), stats::summary(&gaps)) {
        println!(
            "{name} scroll by {}: {:.1} frames/s, gaps of {:.1} ms at the median and {:.1} at \
             the 95th percentile, of {} rounds",
            conditions["by"].as_str().unwrap_or_default(),
            frames.median,
            gaps.median,
            gaps.p95,
            frames.count
        );
    }
    for (metric, unit, samples) in [
        ("frames_per_s", "frames/s", frames),
        ("frame_gap_ms", "ms", gaps),
    ] {
        session
            .append(&Record {
                tool: "scroll".into(),
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

/// A round: the terminal launched printing its lines, scrolled, then quit.
fn round(
    name: &str,
    terminal: &Terminal,
    binary: &std::path::Path,
    home: &std::path::Path,
    settings: Settings,
) -> Result<Figures, String> {
    let _awake = settings.awake.then(Awake::start);
    let payload =
        Payload::new(binary, &pattern::PAYLOAD).map_err(|error| format!("the payload: {error}"))?;
    let (launched, _) = terminal::launch_ready(name, terminal, &payload, home, TIMEOUT)?;
    let measured = payload
        .wait("done", TIMEOUT)
        .ok_or_else(|| format!("{name} printed its lines in no {} s", TIMEOUT.as_secs()))
        .and_then(|_| measure(name, &launched, settings));
    launched
        .quit(QUIT)
        .map_err(|error| format!("{name} does not quit: {error}"))?;
    measured
}

/// The scroll of a terminal launched, its lines printed, measured.
fn measure(
    name: &str,
    launched: &terminal::Launched,
    settings: Settings,
) -> Result<Figures, String> {
    launched
        .wait_window(TIMEOUT)
        .ok_or_else(|| format!("{name} showed no window"))?;
    thread::sleep(SETTLE);
    let window = launched
        .window()
        .ok_or_else(|| format!("{name}'s window went"))?;
    let capture = Capture::start(window.id, settings.source)?;
    let (width, height) = capture.size();
    // A band of the text's middle from the window's left, of the lines'
    // numbers, which every scroll moves, by rows too.
    let band = Rect {
        x: 0,
        y: height * 7 / 20,
        width: width * 4 / 10,
        height: height * 3 / 10,
    };
    capture.band(Some(band));
    let (x, y) = (
        window.frame.x + window.frame.width / 2.0,
        window.frame.y + window.frame.height / 2.0,
    );
    launched.front_at(x, y, FRONT)?;
    pattern::away(x, y);
    let mut last = None;
    while let Some(frame) = capture.try_next() {
        last = Some(frame);
    }
    let events = pattern::plan(settings.pattern);
    let tick = settings.pattern.tick();
    let (done, ended) = mpsc::channel();
    let scroller = thread::spawn(move || {
        let span = pattern::post(x, y, &events, tick);
        let _ = done.send(());
        span
    });
    let mut shown = Vec::new();
    loop {
        let finished = ended.try_recv().is_ok();
        if let Some(frame) = capture.next(Duration::from_millis(50)) {
            let moved = last
                .as_ref()
                .is_none_or(|before: &Frame| frames::changed_box(before, &frame, None).is_some());
            if moved {
                shown.push(frame.display_ns);
            }
            last = Some(frame);
        }
        if finished {
            break;
        }
    }
    let (start, end) = scroller
        .join()
        .map_err(|_| "the scroll's thread failed".to_owned())?;
    // The frames that the scroll's last events moved come after them.
    let until = Instant::now() + Duration::from_millis(100);
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        let Some(frame) = capture.next(left) else {
            break;
        };
        if last
            .as_ref()
            .is_none_or(|before| frames::changed_box(before, &frame, None).is_some())
        {
            shown.push(frame.display_ns);
        }
        last = Some(frame);
    }
    figures(&shown, start, end).ok_or_else(|| format!("{name} showed no frame of the scroll"))
}

/// The figures of frames taken at `shown`, of the scroll's span from
/// `start` to `end`: the frames within it a second, and the gaps between
/// them; none of no frame.
fn figures(shown: &[u64], start: u64, end: u64) -> Option<Figures> {
    let within: Vec<u64> = shown
        .iter()
        .copied()
        .filter(|&at| at >= start && at <= end)
        .collect();
    if within.is_empty() || end <= start {
        return None;
    }
    let seconds = (end - start) as f64 / 1e9;
    Some(Figures {
        frames_per_s: within.len() as f64 / seconds,
        gaps_ms: within
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) as f64 / 1e6)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frames_of_a_span_count_a_second_and_their_gaps() {
        let period = 1e9 / 120.0;
        let start = 1_000_000_000_u64;
        // A frame each refresh for two seconds, and one before the span.
        let shown: Vec<u64> = std::iter::once(start - 5_000_000)
            .chain((1..=240).map(|at| start + (at as f64 * period) as u64))
            .collect();
        let all = figures(&shown, start, start + 2_000_000_000).unwrap();
        assert!((all.frames_per_s - 120.0).abs() < 0.6, "{all:?}");
        assert_eq!(all.gaps_ms.len(), 239);
        assert!(all.gaps_ms.iter().all(|&gap| (gap - 8.33).abs() < 0.01));
        assert_eq!(figures(&[], 0, 1), None);
    }
}
