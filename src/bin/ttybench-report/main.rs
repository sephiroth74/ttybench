//! `ttybench-report`: a session's report. Of a session's folder, its
//! records, its machine, the coordinator's log and the session's file,
//! each metric of each tool and conditions, a table of its terminals'
//! medians and spreads; the A/B of two of its terminals, each metric's
//! change and its interval; and its budgets, each told held or not; in
//! Markdown, `report.md`, and in JSON, `report.json`, of the same
//! figures, which the charts and someone else's tools read.
//!
//! ```text
//! ttybench report <folder> [--a <terminal> --b <terminal>]
//!                 [--out <file>|-] [--json <file>] [--style <file>]
//! ```
//!
//! `--a` and `--b` compare two terminals in place of the session's `ab`.
//! `--style` takes a style in place of the session's, its tables'
//! columns, titles and units and each metric's statistic of the A/B.
//! `--out -` writes the Markdown to the standard output, and the JSON only
//! of `--json`.

mod analysis;
mod markdown;
mod metrics;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ttybench::budget::{self, Ab, Judged};
use ttybench::cli::Args;
use ttybench::config;
use ttybench::results::{self, MACHINE, Machine, RECORDS};
use ttybench::style::Style;

/// The session's file, in its folder, as the coordinator copies it.
const SESSION: &str = "session.toml";

/// The report's Markdown, in the session's folder.
const MARKDOWN: &str = "report.md";

/// The report's JSON, in the session's folder.
const JSON: &str = "report.json";

/// How the report runs.
const USAGE: &str = "ttybench report <folder> [--a <terminal> --b <terminal>] [--out <file>|-] [--json <file>] [--style <file>]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-report: {why}");
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
    if args.first().is_none_or(|arg| arg.starts_with("--")) {
        return Err(format!("which session's folder?\n{USAGE}"));
    }
    let folder = PathBuf::from(config::expand_home(&args.remove(0)));
    let options = Args::parse(args, &["a", "b", "out", "json", "style"], &[])?;
    let given = options
        .value("style")
        .map(|path| PathBuf::from(config::expand_home(path)));
    let style = Style::of_session(&folder, given.as_deref())?;
    let records = results::read_records(&folder.join(RECORDS)).map_err(|error| {
        format!(
            "{} holds no session's records: {error}",
            folder.join(RECORDS).display()
        )
    })?;
    let machine = read_machine(&folder)?;
    let log = results::read_log(&folder).map_err(|error| format!("the runs' log: {error}"))?;
    let mut judged: Judged = match folder.join(SESSION) {
        file if file.exists() => config::read(&file).map_err(|error| error.to_string())?,
        _ => Judged::default(),
    };
    match (options.value("a"), options.value("b")) {
        (Some(a), Some(b)) => {
            judged.ab = Some(Ab {
                a: a.to_owned(),
                b: b.to_owned(),
            });
        }
        (None, None) => {}
        _ => return Err("--a and --b go together, the copy before and the one after".into()),
    }
    let mut terminals = judged.terminals.clone();
    for record in &records {
        if !terminals.contains(&record.terminal) {
            terminals.push(record.terminal.clone());
        }
    }
    budget::check(judged.ab.as_ref(), &judged.budgets, &terminals)?;
    let session = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.clone())
        .file_name()
        .map_or_else(
            || folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
    let report = analysis::report(&analysis::Input {
        session,
        machine,
        records: &records,
        log: &log,
        terminals: &judged.terminals,
        ab: judged.ab.clone(),
        budgets: &judged.budgets,
        style: &style,
    });
    let text = markdown::render(&report, &style);
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())? + "\n";
    let out = options.value("out");
    if out == Some("-") {
        print!("{text}");
    } else {
        let path = out.map_or_else(|| folder.join(MARKDOWN), PathBuf::from);
        write(&path, &text)?;
        eprintln!("ttybench-report: {}", path.display());
    }
    let json_path = match (options.value("json"), out) {
        (Some(path), _) => Some(PathBuf::from(path)),
        (None, Some("-")) => None,
        (None, _) => Some(folder.join(JSON)),
    };
    if let Some(path) = json_path {
        write(&path, &json)?;
        eprintln!("ttybench-report: {}", path.display());
    }
    let not_held = report
        .judgements
        .iter()
        .filter(|judgement| judgement.held == Some(false))
        .count();
    if !report.judgements.is_empty() {
        let held = report
            .judgements
            .iter()
            .filter(|judgement| judgement.held == Some(true))
            .count();
        eprintln!("ttybench-report: {held} budgets held, {not_held} not held");
    }
    Ok(())
}

/// The machine of the session in `folder`, if it tells one.
fn read_machine(folder: &Path) -> Result<Option<Machine>, String> {
    let path = folder.join(MACHINE);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
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
