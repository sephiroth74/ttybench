//! `ttybench-gpu`: how much of the GPU a terminal uses. Each round launches
//! the terminal running the payload, at rest or of lines to scroll through;
//! then, over `seconds` of rest or of a scroll as `ttybench-scroll`'s,
//! reads the GPU time of the terminal's processes and of WindowServer,
//! which composes the windows on the display, of the GPU's clients that
//! IOKit lists; and, of `--system`, samples the system's GPU, its power,
//! its frequency and the share of the time that it worked, of
//! `powermetrics`.
//!
//! ```text
//! ttybench-gpu --terminal <name> [--terminals <file>] [--feature <name>] [--workload rest|scroll]
//!              [--seconds <n>] [--by pixels|lines] [--system]
//!              [--interval <ms>] [--awake on|off] [--rounds <n>]
//!              [--pause <seconds>] [--session <folder>]
//! ```
//!
//! The processes' GPU time takes no privilege. `--system` takes `sudo`,
//! through `sudo -n`, of credentials already given: run `sudo -v` first, or
//! a session of `sudo = true`. The scroll takes the Accessibility
//! permission of the app that runs the tool; leave the mouse and the
//! trackpad alone meanwhile.

use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use ttybench_core::awake::Awake;
use ttybench_core::cli::{self, Args};
use ttybench_core::clock::now_ns;
use ttybench_core::config::Terminal;
use ttybench_core::gpu::{self, Powermetrics};
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Machine, Record, Session};
use ttybench_core::scroll::{self as pattern, By, Pattern};
use ttybench_core::{stats, terminal};

/// How long a round waits for the terminal's window, its payload and its
/// lines.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long a terminal takes to quit, at most.
const QUIT: Duration = Duration::from_secs(5);

/// How long the window rests once it shows, before the measure.
const SETTLE: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-gpu: {why}");
            ExitCode::FAILURE
        }
    }
}

/// What the terminal does while the GPU is read.
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
    system: bool,
    interval: Duration,
    awake: bool,
}

/// A round's figures: the GPU time of the terminal's processes and of
/// WindowServer, in milliseconds a second, and the system's GPU, of
/// `--system`: its power in milliwatts, the share of the time that it
/// worked in percent, and its frequency while it worked in megahertz.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Figures {
    terminal_ms_per_s: f64,
    window_server_ms_per_s: f64,
    system: Option<(f64, f64, f64)>,
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
            "interval",
            "awake",
            "rounds",
            "pause",
            "session",
        ],
        &["system"],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let seconds: f64 = args.number("seconds", 10.0)?;
    let workload = match args.value("workload").unwrap_or("rest") {
        "rest" => Workload::Rest,
        "scroll" => {
            let by = match args.value("by").unwrap_or("pixels") {
                "pixels" => By::Pixels,
                "lines" => By::Lines,
                other => return Err(format!("--by takes pixels or lines, not `{other}`")),
            };
            Workload::Scroll(Pattern {
                by,
                seconds,
                rate: by.rate(),
                amplitude: 200.0,
            })
        }
        other => return Err(format!("--workload takes rest or scroll, not `{other}`")),
    };
    if matches!(workload, Workload::Scroll(_)) {
        ttybench_core::keys::may_post()?;
    }
    if seconds < pattern::PERIOD {
        return Err(format!("--seconds takes {} at least", pattern::PERIOD));
    }
    let settings = Settings {
        workload,
        seconds,
        system: args.flag("system"),
        interval: Duration::from_millis(args.number("interval", 250)?),
        awake: match args.value("awake").unwrap_or("on") {
            "on" => true,
            "off" => false,
            other => return Err(format!("--awake takes on or off, not `{other}`")),
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
        let figures = round(&name, &terminal, &binary, &home, settings)?;
        let system = figures
            .system
            .map_or(String::new(), |(power, active, freq)| {
                format!("; the GPU {power:.0} mW, {active:.1} % active at {freq:.0} MHz")
            });
        eprintln!(
            "{name} round {}/{rounds}: its GPU time {:.2} ms/s, WindowServer's {:.2} ms/s{system}",
            at + 1,
            figures.terminal_ms_per_s,
            figures.window_server_ms_per_s,
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
    let mut metrics: Vec<(&str, &str, Vec<f64>)> = vec![
        (
            "terminal_gpu_ms_per_s",
            "ms/s",
            measured.iter().map(|f| f.terminal_ms_per_s).collect(),
        ),
        (
            "window_server_gpu_ms_per_s",
            "ms/s",
            measured.iter().map(|f| f.window_server_ms_per_s).collect(),
        ),
    ];
    if settings.system {
        let system: Vec<(f64, f64, f64)> = measured.iter().filter_map(|f| f.system).collect();
        metrics.push(("gpu_power_mw", "mW", system.iter().map(|s| s.0).collect()));
        metrics.push((
            "gpu_active_percent",
            "%",
            system.iter().map(|s| s.1).collect(),
        ));
        metrics.push(("gpu_freq_mhz", "MHz", system.iter().map(|s| s.2).collect()));
    }
    for (metric, unit, samples) in metrics {
        if let Some(summary) = stats::summary(&samples) {
            println!(
                "{name} {metric}: median {:.2} {unit}, of {} rounds",
                summary.median, summary.count
            );
        }
        session
            .append(&Record {
                tool: "gpu".into(),
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

/// A round: the terminal launched, its GPU read over its workload, then
/// quit.
fn round(
    name: &str,
    terminal: &Terminal,
    binary: &std::path::Path,
    home: &std::path::Path,
    settings: Settings,
) -> Result<Figures, String> {
    let _awake = settings.awake.then(Awake::start);
    let workload: &[&str] = match settings.workload {
        Workload::Rest => &["rest"],
        Workload::Scroll(_) => &pattern::PAYLOAD,
    };
    let payload =
        Payload::new(binary, workload).map_err(|error| format!("the payload: {error}"))?;
    let (launched, _) = terminal::launch_ready(name, terminal, &payload, home, TIMEOUT)?;
    let measured = match settings.workload {
        Workload::Rest => Ok(()),
        Workload::Scroll(_) => payload
            .wait("done", TIMEOUT)
            .map(|_| ())
            .ok_or_else(|| format!("{name} printed its lines in no {} s", TIMEOUT.as_secs())),
    }
    .and_then(|()| measure(name, &launched, settings));
    launched
        .quit(QUIT)
        .map_err(|error| format!("{name} does not quit: {error}"))?;
    measured
}

/// The GPU of a terminal launched, over its workload.
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
    let (x, y) = (
        window.frame.x + window.frame.width / 2.0,
        window.frame.y + window.frame.height / 2.0,
    );
    if matches!(settings.workload, Workload::Scroll(_)) {
        pattern::away(x, y);
    }
    let server = gpu::window_server().ok_or("no WindowServer runs")?;
    let sampler = settings
        .system
        .then(|| Powermetrics::start(settings.interval))
        .transpose()?;
    // powermetrics takes a sample's length to give its first.
    if sampler.is_some() {
        thread::sleep(settings.interval * 2);
    }
    let before = gpu::process_times()?;
    let start = now_ns();
    let end = match settings.workload {
        Workload::Rest => {
            thread::sleep(Duration::from_secs_f64(settings.seconds));
            now_ns()
        }
        Workload::Scroll(scroll) => {
            let events = pattern::plan(scroll);
            pattern::post(x, y, &events, scroll.tick()).1.max(now_ns())
        }
    };
    let after = gpu::process_times()?;
    let samples = match sampler {
        Some(sampler) => {
            thread::sleep(settings.interval * 2);
            Some(sampler.stop()?)
        }
        None => None,
    };
    let seconds = end.saturating_sub(start) as f64 / 1e9;
    let tree = launched.tree();
    let per_second = |ns: u64| ns as f64 / 1e6 / seconds;
    let system = match samples {
        Some(samples) => Some(
            gpu::system_over(&samples, start, end)
                .ok_or("powermetrics gave no sample of the measure")?,
        ),
        None => None,
    };
    Ok(Figures {
        terminal_ms_per_s: per_second(gpu::used_between(&before, &after, &tree)),
        window_server_ms_per_s: per_second(gpu::used_between(&before, &after, &[server])),
        system,
    })
}
