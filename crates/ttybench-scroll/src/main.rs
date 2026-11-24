//! `ttybench-scroll`: how smoothly a terminal scrolls. Each round launches
//! the terminal running the payload, which prints lines enough for a long
//! scrollback; captures the terminal's window; scrolls it, as a trackpad
//! does, of precise pixels, its fingers' swipes and their momentum, or as a
//! mouse wheel does, of lines; and counts the frames that the window server
//! took of the text moved: the frames a second, and the gaps between them.
//!
//! ```text
//! ttybench-scroll --terminal <name> [--terminals <file>] [--by pixels|lines]
//!                 [--seconds <n>] [--rate <n>] [--amplitude <points>]
//!                 [--source window|screen] [--presence on|off]
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

use ttybench_core::capture::{Capture, Source};
use ttybench_core::cli::{self, Args};
use ttybench_core::clock::now_ns;
use ttybench_core::config::Terminal;
use ttybench_core::frames::{self, Frame, Rect};
use ttybench_core::keys::{self, Scroll};
use ttybench_core::payload::{self, Payload};
use ttybench_core::presence::Presence;
use ttybench_core::results::{self, Machine, Record, Session};
use ttybench_core::{stats, terminal};

/// How long a round waits for the terminal's window, its payload and its
/// lines.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long a terminal takes to quit, at most.
const QUIT: Duration = Duration::from_secs(5);

/// How long the window rests once its lines are printed, before its
/// capture.
const SETTLE: Duration = Duration::from_secs(2);

/// The lines that the payload prints, and their width, which the window's
/// columns hold.
const LINES: &str = "50000";
const WIDTH: &str = "60";

/// The period of the scroll's pattern, up then down.
const PERIOD: f64 = 2.0;

/// How long before an event the scroll's thread stops sleeping and spins,
/// past a sleep's usual lateness.
const SPIN: Duration = Duration::from_micros(1500);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-scroll: {why}");
            ExitCode::FAILURE
        }
    }
}

/// How the scroll comes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum By {
    /// A trackpad's precise pixels, of phases and momentum.
    Pixels,
    /// A wheel's lines, a step at a time.
    Lines,
}

/// A round's settings.
#[derive(Clone, Copy, Debug)]
struct Settings {
    by: By,
    seconds: f64,
    rate: f64,
    amplitude: f64,
    source: Source,
    presence: bool,
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
            "by",
            "seconds",
            "rate",
            "amplitude",
            "rounds",
            "pause",
            "session",
            "source",
            "presence",
        ],
        &[],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let by = match args.value("by").unwrap_or("pixels") {
        "pixels" => By::Pixels,
        "lines" => By::Lines,
        other => return Err(format!("--by takes pixels or lines, not `{other}`")),
    };
    let settings = Settings {
        by,
        seconds: args.number("seconds", 10.0)?,
        rate: args.number(
            "rate",
            match by {
                By::Pixels => 120.0,
                By::Lines => 20.0,
            },
        )?,
        amplitude: args.number("amplitude", 200.0)?,
        source: match args.value("source").unwrap_or("window") {
            "screen" => Source::Screen,
            "window" => Source::Window,
            other => return Err(format!("--source takes screen or window, not `{other}`")),
        },
        presence: match args.value("presence").unwrap_or("on") {
            "on" => true,
            "off" => false,
            other => return Err(format!("--presence takes on or off, not `{other}`")),
        },
    };
    if settings.seconds < PERIOD || settings.rate < 4.0 {
        return Err(format!(
            "--seconds takes {PERIOD} at least, and --rate 4 at least"
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
    conditions.insert("rounds".into(), rounds.into());
    conditions.insert(
        "by".into(),
        match settings.by {
            By::Pixels => "pixels",
            By::Lines => "lines",
        }
        .into(),
    );
    conditions.insert("seconds".into(), settings.seconds.into());
    conditions.insert("presence".into(), settings.presence.into());
    conditions.insert(
        "source".into(),
        match settings.source {
            Source::Screen => "screen",
            Source::Window => "window",
        }
        .into(),
    );
    conditions.insert("rate".into(), settings.rate.into());
    if settings.by == By::Pixels {
        conditions.insert("amplitude".into(), settings.amplitude.into());
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
    let _presence = settings.presence.then(Presence::start);
    let payload = Payload::new(binary, &["lines", "--count", LINES, "--width", WIDTH])
        .map_err(|error| format!("the payload: {error}"))?;
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
    keys::point(x, y);
    thread::sleep(Duration::from_millis(100));
    // Away from the scrollback's end first, so that the scroll down meets
    // none.
    for step in 0..12 {
        keys::scroll(
            x,
            y,
            Scroll::Pixels {
                pixels: 170,
                phase: if step == 0 { 1 } else { 2 },
                momentum: 0,
            },
        );
        thread::sleep(Duration::from_millis(8));
    }
    keys::scroll(
        x,
        y,
        Scroll::Pixels {
            pixels: 0,
            phase: 4,
            momentum: 0,
        },
    );
    thread::sleep(Duration::from_secs(1));
    let mut last = None;
    while let Some(frame) = capture.try_next() {
        last = Some(frame);
    }
    let events = plan(settings);
    let tick = Duration::from_secs_f64(1.0 / settings.rate);
    let (done, ended) = mpsc::channel();
    let scroller = thread::spawn(move || {
        let span = post(x, y, &events, tick);
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

/// The scroll's events, in their order, `rate` a second, as `settings`
/// says.
fn plan(settings: Settings) -> Vec<Scroll> {
    let periods = (settings.seconds / PERIOD).round().max(1.0) as usize;
    let mut events = Vec::new();
    match settings.by {
        By::Pixels => {
            let per = (PERIOD / 4.0 * settings.rate).round().max(2.0) as usize;
            let fingers = settings.amplitude / per as f64;
            for _ in 0..periods {
                for sign in [1.0, -1.0] {
                    for step in 0..per {
                        events.push(Scroll::Pixels {
                            pixels: (sign * fingers).round() as i32,
                            phase: if step == 0 { 1 } else { 2 },
                            momentum: 0,
                        });
                    }
                    events.push(Scroll::Pixels {
                        pixels: 0,
                        phase: 4,
                        momentum: 0,
                    });
                    for step in 0..per {
                        let left = 1.0 - step as f64 / per as f64;
                        // Down to a pixel an event, never none, so that the
                        // view moves throughout.
                        let pixels = (sign * (fingers * left * left).round().max(1.0)) as i32;
                        let momentum = match step {
                            0 => 1,
                            _ if step + 1 == per => 3,
                            _ => 2,
                        };
                        events.push(Scroll::Pixels {
                            pixels,
                            phase: 0,
                            momentum,
                        });
                    }
                }
            }
        }
        By::Lines => {
            let per = (PERIOD / 2.0 * settings.rate).round().max(1.0) as usize;
            for _ in 0..periods {
                for sign in [1, -1] {
                    events.extend(std::iter::repeat_n(Scroll::Lines(sign), per));
                }
            }
        }
    }
    events
}

/// Posts `events` at `x`, `y`, `tick` apart, on time as a trackpad's come:
/// the thread of the interactive class, which the system's timers do not
/// coalesce, asleep until a moment before each event, then spinning to it.
/// The span of the scroll, from its first event to a tick past its last,
/// in nanoseconds of the shared clock.
fn post(x: f64, y: f64, events: &[Scroll], tick: Duration) -> (u64, u64) {
    // SAFETY: a call on the calling thread, of a class of the system's.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
    let origin = Instant::now();
    let start = now_ns();
    for (at, &event) in events.iter().enumerate() {
        let due = origin + tick.mul_f64(at as f64);
        if let Some(wait) = due
            .checked_duration_since(Instant::now())
            .and_then(|wait| wait.checked_sub(SPIN))
        {
            thread::sleep(wait);
        }
        while Instant::now() < due {
            std::hint::spin_loop();
        }
        keys::scroll(x, y, event);
    }
    let end = start + u64::try_from(tick.mul_f64(events.len() as f64).as_nanos()).unwrap_or(0);
    (start, end)
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

    fn settings(by: By) -> Settings {
        Settings {
            by,
            seconds: 4.0,
            rate: 120.0,
            amplitude: 200.0,
            source: Source::Screen,
            presence: true,
        }
    }

    #[test]
    fn a_trackpads_scroll_swipes_lets_its_momentum_slow_and_comes_back() {
        let events = plan(settings(By::Pixels));
        // Two periods, each of two swipes of 60 events and their end, and
        // two momentums of 60.
        assert_eq!(events.len(), 2 * 2 * (60 + 1 + 60));
        let sum: i32 = events
            .iter()
            .map(|event| match event {
                Scroll::Pixels { pixels, .. } => *pixels,
                Scroll::Lines(_) => 0,
            })
            .sum();
        assert_eq!(sum, 0);
        assert_eq!(
            events[0],
            Scroll::Pixels {
                pixels: 3,
                phase: 1,
                momentum: 0
            }
        );
        assert_eq!(
            events[60],
            Scroll::Pixels {
                pixels: 0,
                phase: 4,
                momentum: 0
            }
        );
        assert!(matches!(events[61], Scroll::Pixels { momentum: 1, .. }));
        // The momentum slows to a pixel, never none.
        assert!(matches!(
            events[120],
            Scroll::Pixels {
                pixels: 1,
                momentum: 3,
                ..
            }
        ));
    }

    #[test]
    fn a_wheels_scroll_steps_up_then_down() {
        let events = plan(Settings {
            rate: 20.0,
            ..settings(By::Lines)
        });
        assert_eq!(events.len(), 2 * 2 * 20);
        assert_eq!(events[19], Scroll::Lines(1));
        assert_eq!(events[20], Scroll::Lines(-1));
    }

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
