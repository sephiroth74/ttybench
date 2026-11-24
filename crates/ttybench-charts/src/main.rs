//! `ttybench-charts`: a session's charts, as SVG files that a page or a
//! Markdown file shows, of its report's JSON and its records: bars of each
//! terminal's median and its 95th percentile of each metric; the latency's
//! distribution, whole and of its slowest tenth; the gaps between frames
//! in time; the GPU's time of each terminal on WindowServer's; and the
//! A/B's change of each metric, its interval and its budgets. Each chart
//! follows the reader's light or dark appearance, tells its values as the
//! pointer rests on a mark, and carries a title and a description for
//! screen readers; `charts.json` lists them. Last, the session's page,
//! `index.html`, static, of no script: the report's tables, each chart
//! after its own.
//!
//! Of `--history` and several sessions' folders, the oldest first, each
//! terminal's metrics along its versions, a chart of each terminal and
//! tool, and their page, into `--out`, `ttybench-history` by default.
//!
//! ```text
//! ttybench charts <folder> [--out <folder>]
//! ttybench charts --history <folder> <folder>... [--out <folder>]
//! ```
//!
//! The session's report comes first: `ttybench report <folder>` writes the
//! `report.json` and the `report.md` that this reads.

mod history;
mod kinds;
mod model;
mod page;
mod svg;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use ttybench_core::cli::Args;
use ttybench_core::config;
use ttybench_core::results::{self, RECORDS};

/// The report's JSON, in the session's folder.
const REPORT: &str = "report.json";

/// The charts' folder, in the session's.
const CHARTS: &str = "charts";

/// The list of the charts, in their folder.
const INDEX: &str = "charts.json";

/// The report's Markdown, in the session's folder.
const MARKDOWN: &str = "report.md";

/// A session's page, in its folder, and the history's, in its own.
const PAGE: &str = "index.html";

/// The history's folder, unless `--out` names one.
const HISTORY: &str = "ttybench-history";

/// How the charts run.
const USAGE: &str = "ttybench charts <folder> [--out <folder>]\n       ttybench charts --history <folder> <folder>... [--out <folder>]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-charts: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "--history") {
        args.remove(0);
        let count = args.iter().take_while(|arg| !arg.starts_with("--")).count();
        let folders: Vec<PathBuf> = args
            .drain(..count)
            .map(|folder| PathBuf::from(config::expand_home(&folder)))
            .collect();
        let options = Args::parse(args, &["out"], &[])?;
        return history(&folders, options.value("out"));
    }
    if args.first().is_none_or(|arg| arg.starts_with("--")) {
        return Err(format!("which session's folder?\n{USAGE}"));
    }
    let folder = PathBuf::from(config::expand_home(&args.remove(0)));
    let options = Args::parse(args, &["out"], &[])?;
    let report = read_report(&folder)?;
    let records = results::read_records(&folder.join(RECORDS))
        .map_err(|error| format!("{}: {error}", folder.join(RECORDS).display()))?;
    let out = options.value("out").map_or_else(
        || folder.join(CHARTS),
        |out| PathBuf::from(config::expand_home(out)),
    );
    let drawn = kinds::all(&report, &records);
    write_charts(&out, &drawn)?;
    eprintln!(
        "ttybench-charts: {} charts of {} in {}",
        drawn.len(),
        report.session,
        out.display()
    );
    // The page beside the report, of the charts in their folder, unless
    // they went elsewhere.
    if options.value("out").is_none() {
        let markdown = std::fs::read_to_string(folder.join(MARKDOWN))
            .map_err(|error| format!("{}: {error}", folder.join(MARKDOWN).display()))?;
        let title = format!("ttybench session {}", report.session);
        write(
            &folder.join(PAGE),
            &page::session(&title, &markdown, &drawn),
        )?;
        eprintln!("ttybench-charts: {}", folder.join(PAGE).display());
    }
    Ok(())
}

/// The history of the sessions in `folders`, the oldest first, into
/// `out`, or `ttybench-history` of none.
fn history(folders: &[PathBuf], out: Option<&str>) -> Result<(), String> {
    if folders.len() < 2 {
        return Err(format!(
            "a history takes two sessions' folders at least\n{USAGE}"
        ));
    }
    let sessions = folders
        .iter()
        .map(|folder| read_report(folder))
        .collect::<Result<Vec<_>, _>>()?;
    let out = PathBuf::from(config::expand_home(out.unwrap_or(HISTORY)));
    let drawn = history::history(&sessions);
    if drawn.is_empty() {
        return Err("no terminal is of two of these sessions: no history".to_owned());
    }
    write_charts(&out, &drawn)?;
    let names: Vec<&str> = sessions
        .iter()
        .map(|report| report.session.as_str())
        .collect();
    write(
        &out.join(PAGE),
        &page::history(&format!("ttybench history of {}", names.join(", ")), &drawn),
    )?;
    eprintln!(
        "ttybench-charts: {} charts of the history of {} sessions in {}",
        drawn.len(),
        sessions.len(),
        out.display()
    );
    Ok(())
}

/// The report of the session in `folder`.
fn read_report(folder: &Path) -> Result<model::Report, String> {
    let text = std::fs::read_to_string(folder.join(REPORT)).map_err(|error| {
        format!(
            "{}: {error}; `ttybench report {}` writes it",
            folder.join(REPORT).display(),
            folder.display()
        )
    })?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{}: {error}", folder.join(REPORT).display()))
}

/// Writes `drawn` into `out`, of their list, and takes away the charts of
/// an earlier run that it did not draw again.
fn write_charts(out: &Path, drawn: &[kinds::Drawn]) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let stale = listed(&out.join(INDEX));
    for chart in drawn {
        write(&out.join(&chart.file), &chart.svg)?;
    }
    // The charts of an earlier run that this one did not draw again.
    for file in stale {
        if !drawn.iter().any(|chart| chart.file == file) && is_chart_file(&file) {
            let _ = std::fs::remove_file(out.join(&file));
        }
    }
    let index = serde_json::to_string_pretty(&drawn).map_err(|error| error.to_string())? + "\n";
    write(&out.join(INDEX), &index)
}

/// The files that the list at `path` names, of an earlier run; none of
/// none.
fn listed(path: &Path) -> Vec<String> {
    #[derive(Deserialize)]
    struct Listed {
        file: String,
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Listed>>(&text).ok())
        .map(|listed| listed.into_iter().map(|chart| chart.file).collect())
        .unwrap_or_default()
}

/// Whether `file` can be a chart's file: an SVG's name of no folder.
fn is_chart_file(file: &str) -> bool {
    file.ends_with(".svg") && !file.contains('/') && !file.starts_with('.')
}

/// Writes `text` to `path`, whole or not at all.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} names no file", path.display()))?;
    let temporary = path.with_file_name(format!(".{}.new", name.to_string_lossy()));
    std::fs::write(&temporary, text)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| format!("{}: {error}", path.display()))
}
