//! `ttybench-idle`: what a terminal uses at rest, or while its payload
//! runs a workload. Each round launches the terminal running the payload,
//! lets it settle, then reads its processes' counters at each interval:
//! the CPU's share, the wakeups that bring a CPU package out of idle and
//! those of interrupts, a second each, and the power that the kernel bills
//! them. A terminal at rest should take none of them.
//!
//! ```text
//! ttybench-idle --terminal <name> [--terminals <file>] [--rounds <n>]
//!               [--duration <seconds>] [--interval <seconds>]
//!               [--settle <seconds>] [--pause <seconds>]
//!               [--workload "<payload's workload>"] [--session <folder>]
//! ```

use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use ttybench_core::cli::{self, Args};
use ttybench_core::clock::now_ns;
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Record, Session};
use ttybench_core::{process, stats, terminal};

/// How long a round waits for the payload.
const TIMEOUT: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-idle: {why}");
            ExitCode::FAILURE
        }
    }
}

/// The metrics, their units, and each interval's value of what the
/// processes used over its seconds.
const METRICS: [(&str, &str); 4] = [
    ("cpu_percent", "%"),
    ("package_idle_wakeups_per_s", "/s"),
    ("interrupt_wakeups_per_s", "/s"),
    ("power_mw", "mW"),
];

fn run() -> Result<(), String> {
    let args = Args::parse(
        std::env::args().skip(1),
        &[
            "terminal",
            "terminals",
            "rounds",
            "duration",
            "interval",
            "settle",
            "pause",
            "workload",
            "session",
        ],
        &[],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let rounds: usize = args.number("rounds", 1)?;
    let duration: f64 = args.number("duration", 10.0)?;
    let interval: f64 = args.number("interval", 1.0)?;
    if interval <= 0.0 || duration < interval {
        return Err("--duration covers one --interval at least".to_owned());
    }
    let settle = Duration::from_secs_f64(args.number("settle", 3.0)?);
    let pause = Duration::from_secs_f64(args.number("pause", 2.0)?);
    let workload: Vec<&str> = args
        .value("workload")
        .unwrap_or("rest")
        .split_whitespace()
        .collect();
    let session = Session::open(std::path::Path::new(
        args.value("session").unwrap_or("ttybench-session"),
    ))
    .map_err(|error| format!("the session's folder: {error}"))?;
    let binary = payload::binary()?;
    let home = std::env::home_dir().unwrap_or_else(std::env::temp_dir);
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); METRICS.len()];
    for round in 0..rounds {
        let payload =
            Payload::new(&binary, &workload).map_err(|error| format!("the payload: {error}"))?;
        let (launched, _) = terminal::launch_ready(&name, &terminal, &payload, &home, TIMEOUT)?;
        thread::sleep(settle);
        let steps = (duration / interval).floor() as usize;
        let mut before = process::usages(&launched.tree());
        let mut at = now_ns();
        for _ in 0..steps {
            thread::sleep(Duration::from_secs_f64(interval));
            let after = process::usages(&launched.tree());
            let now = now_ns();
            let seconds = now.saturating_sub(at) as f64 / 1e9;
            let used = process::used_between(&before, &after);
            let values = [
                used.cpu_ns as f64 / 1e9 / seconds * 100.0,
                used.package_idle as f64 / seconds,
                used.interrupt as f64 / seconds,
                used.energy_nj as f64 / seconds / 1e6,
            ];
            for (kept, value) in samples.iter_mut().zip(values) {
                kept.push(value);
            }
            (before, at) = (after, now);
        }
        launched
            .quit(Duration::from_secs(5))
            .map_err(|error| format!("{name} does not quit: {error}"))?;
        eprintln!("{name} round {}/{rounds} measured", round + 1);
        if round + 1 < rounds {
            thread::sleep(pause);
        }
    }
    let version = results::app_version(&terminal.app);
    let mut conditions = serde_json::Map::new();
    conditions.insert("rounds".into(), rounds.into());
    conditions.insert("duration_s".into(), duration.into());
    conditions.insert("interval_s".into(), interval.into());
    conditions.insert("settle_s".into(), settle.as_secs_f64().into());
    conditions.insert("workload".into(), workload.join(" ").into());
    for ((metric, unit), samples) in METRICS.into_iter().zip(samples) {
        if let Some(summary) = stats::summary(&samples) {
            println!(
                "{name} {metric}: median {:.2} {unit}, p95 {:.2} {unit}, of {} intervals",
                summary.median, summary.p95, summary.count
            );
        }
        session
            .append(&Record {
                tool: "idle".into(),
                terminal: name.clone(),
                version: version.clone(),
                metric: metric.into(),
                unit: unit.into(),
                samples,
                conditions: conditions.clone(),
            })
            .map_err(|error| format!("the session's records: {error}"))?;
    }
    Ok(())
}
