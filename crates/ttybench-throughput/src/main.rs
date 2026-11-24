//! `ttybench-throughput`: how fast a terminal prints. Each round launches
//! the terminal running the payload, which prints a file of `--mib` MiB
//! and tells the time to its last byte's write, as the terminal took it;
//! then the tool waits for the terminal's processes to go quiet, their CPU
//! below a share, as the terminal has drawn what it took. Two rates of
//! each round: to the last write, and to the quiet.
//!
//! ```text
//! ttybench-throughput --terminal <name> [--terminals <file>] [--feature <name>]
//!                     [--text plain|colors|wide] [--mib <n>] [--rounds <n>]
//!                     [--pause <seconds>] [--session <folder>]
//! ```
//!
//! The file of each text and size is written once, into the temporary
//! folder's `ttybench-cache`, and printed again by every run after, so that
//! a session's runs write no GiB each, which the system's file watchers,
//! as an antivirus's, would take the machine for.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use ttybench_core::cli::{self, Args};
use ttybench_core::clock::now_ns;
use ttybench_core::payload::{self, Payload};
use ttybench_core::results::{self, Record, Session};
use ttybench_core::{process, stats, terminal};

/// How long a round waits for the payload, and for its file to print.
const TIMEOUT: Duration = Duration::from_secs(120);

/// How often the quiet is looked for.
const QUIET_STEP: Duration = Duration::from_millis(20);

/// The CPU's share, of one core, below which the terminal is quiet.
const QUIET_PERCENT: f64 = 5.0;

/// How many steps in a row the terminal stays quiet.
const QUIET_STEPS: usize = 3;

/// A mebibyte.
const MIB: u64 = 1024 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-throughput: {why}");
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
            "text",
            "mib",
            "rounds",
            "pause",
            "session",
        ],
        &[],
    )?;
    let (name, terminal) = cli::terminal(&args)?;
    let text = args.value("text").unwrap_or("plain");
    let mib: u64 = args.number("mib", 256)?;
    let rounds: usize = args.number("rounds", 5)?;
    let pause = Duration::from_secs_f64(args.number("pause", 2.0)?);
    let session = Session::open(Path::new(
        args.value("session").unwrap_or("ttybench-session"),
    ))
    .map_err(|error| format!("the session's folder: {error}"))?;
    let file = workload_file(text, mib)?;
    let binary = payload::binary()?;
    let home = std::env::home_dir().unwrap_or_else(std::env::temp_dir);
    let (mut written, mut quiet) = (Vec::new(), Vec::new());
    for round in 0..rounds {
        let path = file.to_string_lossy().into_owned();
        let payload = Payload::new(&binary, &["cat", "--file", &path])
            .map_err(|error| format!("the payload: {error}"))?;
        let (launched, _) = terminal::launch_ready(&name, &terminal, &payload, &home, TIMEOUT)?;
        let Some(done) = payload.wait("done", TIMEOUT) else {
            let _ = launched.quit(Duration::from_secs(5));
            return Err(format!(
                "{name} did not print the file within {} s",
                TIMEOUT.as_secs()
            ));
        };
        let started = done["started_ns"].as_u64().unwrap_or(0);
        let ended = done["time_ns"].as_u64().unwrap_or(0);
        let bytes = done["bytes"].as_u64().unwrap_or(0) as f64;
        let calm = quiet_at(&launched.tree(), ended);
        launched
            .quit(Duration::from_secs(5))
            .map_err(|error| format!("{name} does not quit: {error}"))?;
        let rate = |end: u64| bytes / (MIB as f64) / (end.saturating_sub(started) as f64 / 1e9);
        written.push(rate(ended));
        quiet.push(rate(calm));
        eprintln!(
            "{name} round {}/{rounds}: {:.1} MiB/s to the last write, {:.1} MiB/s to the quiet",
            round + 1,
            rate(ended),
            rate(calm)
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
    conditions.insert("text".into(), text.into());
    conditions.insert("mib".into(), mib.into());
    conditions.insert("rounds".into(), rounds.into());
    for (metric, samples) in [("written_mib_s", written), ("quiet_mib_s", quiet)] {
        if let Some(summary) = stats::summary(&samples) {
            println!(
                "{name} {text} {metric}: median {:.1} MiB/s, of {} rounds",
                summary.median, summary.count
            );
        }
        session
            .append(&Record {
                tool: "throughput".into(),
                terminal: name.clone(),
                version: version.clone(),
                metric: metric.into(),
                unit: "MiB/s".into(),
                samples,
                conditions: conditions.clone(),
                cycle: None,
            })
            .map_err(|error| format!("the session's records: {error}"))?;
    }
    Ok(())
}

/// When the processes `pids` went quiet after `since`, of the shared
/// clock: the start of the first of [`QUIET_STEPS`] steps in a row whose
/// CPU stayed below [`QUIET_PERCENT`]; `since` itself past the timeout.
fn quiet_at(pids: &[libc::pid_t], since: u64) -> u64 {
    let deadline = since + u64::try_from(TIMEOUT.as_nanos()).unwrap_or(u64::MAX);
    let mut before = process::usages(pids);
    let mut at = now_ns();
    let mut calm_since = None;
    let mut calm_steps = 0;
    while now_ns() < deadline {
        thread::sleep(QUIET_STEP);
        let after = process::usages(pids);
        let now = now_ns();
        let used = process::used_between(&before, &after);
        let percent = used.cpu_ns as f64 / now.saturating_sub(at).max(1) as f64 * 100.0;
        if percent < QUIET_PERCENT {
            calm_since.get_or_insert(at);
            calm_steps += 1;
            if calm_steps >= QUIET_STEPS {
                return calm_since.unwrap_or(at).max(since);
            }
        } else {
            calm_since = None;
            calm_steps = 0;
        }
        (before, at) = (after, now);
    }
    since
}

/// The file of `mib` MiB of the text `text`, in the temporary folder's
/// `ttybench-cache`: written once, whole, then kept for the runs after.
fn workload_file(text: &str, mib: u64) -> Result<PathBuf, String> {
    workload_file_in(&std::env::temp_dir().join(CACHE), text, mib)
}

/// The folder of the files that the runs share, in the temporary folder.
const CACHE: &str = "ttybench-cache";

/// The file of `mib` MiB of the text `text` in `folder`, written whole of
/// a name of its own first, then renamed, so that a run cut off leaves
/// none half written, and kept for the runs after: `plain` lines of ASCII,
/// `colors` of a 256 colors' escape before each character, `wide` of CJK
/// ideographs, two cells each.
fn workload_file_in(folder: &Path, text: &str, mib: u64) -> Result<PathBuf, String> {
    let path = folder.join(format!("throughput-{text}-{mib}mib.txt"));
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(folder).map_err(|error| format!("the workload's folder: {error}"))?;
    let line: String = match text {
        "plain" => (0..79)
            .map(|at| char::from(b' ' + 1 + (at % 94) as u8))
            .collect(),
        "colors" => {
            (0..79)
                .map(|at| {
                    format!(
                        "\x1b[38;5;{}m{}",
                        at % 256,
                        char::from(b'a' + (at % 26) as u8)
                    )
                })
                .collect::<String>()
                + "\x1b[0m"
        }
        "wide" => "漢字の幅は二つのセルを占めますこれは広い文字の試験です二十字".repeat(2),
        other => return Err(format!("--text is plain, colors or wide, not `{other}`")),
    };
    let partial = folder.join(format!(
        "throughput-{text}-{mib}mib.txt.{}",
        std::process::id()
    ));
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(&partial).map_err(|error| format!("the workload's file: {error}"))?,
    );
    let mut size = 0_u64;
    while size < mib * MIB {
        writeln!(file, "{line}").map_err(|error| format!("the workload's file: {error}"))?;
        size += line.len() as u64 + 1;
    }
    file.flush()
        .map_err(|error| format!("the workload's file: {error}"))?;
    drop(file);
    std::fs::rename(&partial, &path).map_err(|error| format!("the workload's file: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_text_makes_a_file_of_its_size_and_its_kind() {
        let folder =
            std::env::temp_dir().join(format!("ttybench-throughput-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        for (text, holds) in [
            ("plain", "!\"#$"),
            ("colors", "\x1b[38;5;1mb"),
            ("wide", "漢字"),
        ] {
            let path = workload_file_in(&folder, text, 1).expect(text);
            let written = std::fs::metadata(&path).unwrap().modified().unwrap();
            // Kept: the run after prints the same file, not written again.
            assert_eq!(workload_file_in(&folder, text, 1).unwrap(), path);
            assert_eq!(
                std::fs::metadata(&path).unwrap().modified().unwrap(),
                written
            );
            let contents = std::fs::read_to_string(&path).expect(text);
            assert!(
                contents.len() as u64 >= MIB,
                "{text}: {} bytes",
                contents.len()
            );
            assert!(
                contents
                    .lines()
                    .next()
                    .is_some_and(|line| line.contains(holds)),
                "{text}"
            );
        }
        // No half-written file left beside them.
        let names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|name| name.ends_with("mib.txt")),
            "{names:?}"
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn an_unknown_text_is_refused() {
        assert!(workload_file("emoji", 1).is_err());
    }
}
