//! `ttybench-memory`: how much memory a terminal takes. Each round launches
//! the terminal running the payload, lets it settle, then reads the
//! physical footprint of its processes, as Activity Monitor shows a
//! process's Memory, summed, now and at each one's peak.
//!
//! ```text
//! ttybench-memory --terminal <name> [--terminals <file>] [--feature <name>] [--rounds <n>]
//!                 [--settle <seconds>] [--pause <seconds>]
//!                 [--workload "<payload's workload>"] [--session <folder>]
//! ```

use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use ttybench_core::cli::{self, Args};
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Record, Session};
use ttybench_core::{process, stats, terminal};

/// How long a round waits for the payload.
const TIMEOUT: Duration = Duration::from_secs(30);

/// A mebibyte.
const MIB: f64 = 1024.0 * 1024.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-memory: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(
        std::env::args().skip(1),
        &[
            "terminal",
            "terminals",
            "feature",
            "rounds",
            "settle",
            "pause",
            "workload",
            "session",
        ],
        &[],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let rounds: usize = args.number("rounds", 3)?;
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
    let (mut footprints, mut peaks) = (Vec::new(), Vec::new());
    for round in 0..rounds {
        let payload =
            Payload::new(&binary, &workload).map_err(|error| format!("the payload: {error}"))?;
        let (launched, _) = terminal::launch_ready(&name, &terminal, &payload, &home, TIMEOUT)?;
        thread::sleep(settle);
        let used = process::used_between(&[], &process::usages(&launched.tree()));
        launched
            .quit(Duration::from_secs(5))
            .map_err(|error| format!("{name} does not quit: {error}"))?;
        footprints.push(used.footprint as f64 / MIB);
        peaks.push(used.peak as f64 / MIB);
        eprintln!(
            "{name} round {}/{rounds}: footprint {:.1} MiB, peak {:.1} MiB",
            round + 1,
            used.footprint as f64 / MIB,
            used.peak as f64 / MIB
        );
        if round + 1 < rounds {
            thread::sleep(pause);
        }
    }
    let version = results::app_version(&terminal.app);
    let mut conditions = serde_json::Map::new();
    if let Some(feature) = cli::feature(&args) {
        conditions.insert("feature".into(), feature.into());
    }
    conditions.insert("rounds".into(), rounds.into());
    conditions.insert("settle_s".into(), settle.as_secs_f64().into());
    conditions.insert("workload".into(), workload.join(" ").into());
    for (metric, samples) in [("footprint_mib", footprints), ("peak_mib", peaks)] {
        if let Some(summary) = stats::summary(&samples) {
            println!(
                "{name} {metric}: median {:.1} MiB, of {} rounds",
                summary.median, summary.count
            );
        }
        session
            .append(&Record {
                tool: "memory".into(),
                terminal: name.clone(),
                version: version.clone(),
                metric: metric.into(),
                unit: "MiB".into(),
                samples,
                conditions: conditions.clone(),
                cycle: None,
            })
            .map_err(|error| format!("the session's records: {error}"))?;
    }
    Ok(())
}
