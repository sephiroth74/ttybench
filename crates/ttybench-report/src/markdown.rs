//! A report in Markdown: the session and its machine, its budgets, its
//! A/B, then each tool's metrics, a table of each, and the runs that
//! measured nothing.

use std::fmt::Write;

use serde_json::Value;
use ttybench_core::results::Status;

use crate::analysis::{AbChange, Group, Report, Verdict};

/// The report's Markdown.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# ttybench session {}\n", cell(&report.session));
    let _ = writeln!(out, "{}\n", about(report));
    if !report.terminals.is_empty() {
        out.push_str("| Terminal | Version |\n|---|---|\n");
        for terminal in &report.terminals {
            let _ = writeln!(
                out,
                "| {} | {} |",
                cell(&terminal.name),
                terminal.version.as_deref().map_or("".into(), cell)
            );
        }
        out.push('\n');
    }
    if !report.judgements.is_empty() {
        budgets(&mut out, report);
    }
    if report.ab.is_some() {
        ab(&mut out, report);
    }
    let mut tools: Vec<&str> = Vec::new();
    for group in &report.groups {
        if !tools.contains(&group.tool.as_str()) {
            tools.push(&group.tool);
        }
    }
    for tool in tools {
        let _ = writeln!(out, "## {}\n", cell(tool));
        for group in report.groups.iter().filter(|group| group.tool == tool) {
            table(&mut out, group);
        }
    }
    if !report.runs_log.is_empty() {
        runs(&mut out, report);
    }
    if !report.unmeasured.is_empty() {
        out.push_str("## Runs that measured nothing\n\n");
        for run in &report.unmeasured {
            let how = match run.status {
                Status::Failed => "failed",
                _ => "skipped",
            };
            let _ = writeln!(
                out,
                "- `{}`, {how}: {}",
                run.id.replace('`', "'"),
                cell(&run.why)
            );
        }
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// The runs' table: each run's start, length and end, and the machine's
/// load around it, so that a run beside another program's work tells.
fn runs(out: &mut String, report: &Report) {
    out.push_str("## Runs\n\n");
    out.push_str("Each run's end, the load average of a minute as it started and as it ended, and the busiest processes as it ended, of their share of a core over about the last minute: a run beside another program's work, as a scan of the files, may measure otherwise.\n\n");
    out.push_str(
        "| Run | Started | Seconds | | Load | Busiest as it ended |\n|---|---|---|---|---|---|\n",
    );
    for entry in &report.runs_log {
        let how = match entry.status {
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Skipped => "skipped",
        };
        let (load, busiest) = entry
            .load
            .as_ref()
            .map_or((String::new(), String::new()), |load| {
                let busiest: Vec<String> = load
                    .busiest
                    .iter()
                    .map(|busy| format!("{} {:.0} %", cell(&busy.name), busy.cpu_percent))
                    .collect();
                (
                    format!("{:.2} → {:.2}", load.before, load.after),
                    busiest.join(", "),
                )
            });
        let started = local_seconds(entry.started);
        let _ = writeln!(
            out,
            "| `{}` | {started} | {:.0} | {how} | {load} | {busiest} |",
            entry.id.replace('`', "'"),
            entry.seconds
        );
    }
    out.push('\n');
}

/// The session's line: when, on what, and how its runs ended.
fn about(report: &Report) -> String {
    let mut parts = Vec::new();
    if let Some((from, to)) = report.span {
        let (from, to) = (local(from), local(to));
        let to = match (from.split_once(' '), to.split_once(' ')) {
            (Some((day, _)), Some((other, time))) if day == other => time.to_owned(),
            _ => to,
        };
        parts.push(format!("Measured from {from} to {to}"));
    }
    if let Some(machine) = &report.machine {
        let mut on = format!("on {}", cell(&machine.model));
        if !machine.cpu.is_empty() {
            let _ = write!(on, " ({})", cell(&machine.cpu));
        }
        let _ = write!(on, ", macOS {}", cell(&machine.macos));
        if let Some(hz) = machine.refresh_hz {
            let _ = write!(on, ", the main display at {hz:.0} Hz");
        }
        parts.push(on);
    }
    let mut line = if parts.is_empty() {
        "Measured of a session of no machine told".to_owned()
    } else {
        let mut line = parts.join(" ");
        if let Some(first) = line.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        line
    };
    line.push('.');
    if let Some(runs) = report.runs {
        let total = runs.done + runs.failed + runs.skipped;
        let _ = write!(
            line,
            " {total} {}: {} done, {} failed, {} skipped.",
            if total == 1 { "run" } else { "runs" },
            runs.done,
            runs.failed,
            runs.skipped
        );
    }
    line
}

/// The budgets' table, and how many held.
fn budgets(out: &mut String, report: &Report) {
    out.push_str("## Budgets\n\n");
    let held = report
        .judgements
        .iter()
        .filter(|judgement| judgement.held == Some(true))
        .count();
    let not = report
        .judgements
        .iter()
        .filter(|judgement| judgement.held == Some(false))
        .count();
    let none = report.judgements.len() - held - not;
    let mut line = format!("{held} held, {not} not held");
    if none > 0 {
        let _ = write!(line, ", {none} of nothing to judge");
    }
    let _ = writeln!(out, "{line}.\n");
    out.push_str("| Budget | Of | Value | Limit | |\n|---|---|---|---|---|\n");
    for judgement in &report.judgements {
        let group = judgement.group.map(|at| &report.groups[at]);
        let what = group.map_or_else(
            || format!("{}, its {}", cell(&judgement.metric), judgement.stat.name()),
            |group| {
                format!(
                    "{} · {}, its {}{}",
                    cell(&group.tool),
                    cell(&group.title),
                    judgement.stat.name(),
                    of_conditions(group, true)
                )
            },
        );
        let of = match (&judgement.terminal, &report.ab) {
            (Some(terminal), _) => cell(terminal),
            (None, Some(ab)) if group.is_some() => {
                format!("{} against {}", cell(&ab.b), cell(&ab.a))
            }
            _ => String::new(),
        };
        let unit = group.map_or("", |group| group.unit.as_str());
        let digits = group.map_or(Digits::Fixed(2), Digits::of);
        let value = match (judgement.value, judgement.terminal.is_some()) {
            (Some(value), true) => with_unit(&digits.format(value), unit),
            (Some(change), false) => {
                change_text(change, judgement.percent, digits, unit)
                    + &judgement.interval.map_or(String::new(), |(low, high)| {
                        format!(", 95 % {} … {}", digits.signed(low), digits.signed(high))
                    })
            }
            (None, _) => String::new(),
        };
        let verdict = match judgement.held {
            Some(true) => "✔ held".to_owned(),
            Some(false) => "✘ not held".to_owned(),
            None => cell(judgement.note.as_deref().unwrap_or("nothing to judge")),
        };
        let _ = writeln!(
            out,
            "| {what} | {of} | {value} | {} | {verdict} |",
            cell(&judgement.limit)
        );
    }
    out.push('\n');
}

/// The A/B's table: each metric's medians, their change and its interval.
fn ab(out: &mut String, report: &Report) {
    let Some(ab) = &report.ab else {
        return;
    };
    let _ = writeln!(out, "## A/B: {} against {}\n", cell(&ab.b), cell(&ab.a));
    let _ = writeln!(
        out,
        "Each metric's median of A, {}, and of B, {}; the change, B's less A's, and its 95 % bootstrap interval, of the runs resampled, then their samples, as a run moves as a whole of the machine's state: a change whose interval holds 0 is within the noise, and a copy of fewer than two runs tells too few.\n",
        cell(&ab.a),
        cell(&ab.b)
    );
    out.push_str("| Metric | A | B | Change | 95 % interval | |\n|---|---|---|---|---|---|\n");
    let mut any = false;
    for group in &report.groups {
        let Some(change) = group.change else {
            continue;
        };
        any = true;
        let digits = Digits::of(group);
        let AbChange {
            a,
            b,
            change: by,
            percent,
            interval,
            verdict,
        } = change;
        let verdict = match verdict {
            Verdict::Better => "better",
            Verdict::Worse => "**worse**",
            Verdict::Higher => "higher",
            Verdict::Lower => "lower",
            Verdict::WithinNoise => "within the noise",
            Verdict::TooFew => "too few runs to tell",
        };
        let _ = writeln!(
            out,
            "| {} · {} ({}){} | {} | {} | {} | {} … {} | {verdict} |",
            cell(&group.tool),
            cell(&group.title),
            cell(&group.unit),
            of_conditions(group, true),
            digits.format(a),
            digits.format(b),
            change_text(by, percent, digits, ""),
            digits.signed(interval.0),
            digits.signed(interval.1),
        );
    }
    if !any {
        out.push_str("| No metric of both | | | | | |\n");
    }
    out.push('\n');
}

/// A group's heading, its conditions and its table.
fn table(out: &mut String, group: &Group) {
    let _ = write!(
        out,
        "### {} ({}){}",
        cell(&group.title),
        cell(&group.unit),
        of_conditions(group, true)
    );
    out.push_str("\n\n");
    let mut about = Vec::new();
    if let Some(feature) = &group.feature {
        about.push(format!("Of the feature `{}`.", feature.replace('`', "'")));
    }
    let conditions: Vec<String> = group
        .conditions
        .iter()
        .filter(|(name, _)| name.as_str() != "feature")
        .map(|(name, value)| format!("{} {}", cell(name), cell(&value_text(value))))
        .collect();
    if !conditions.is_empty() {
        about.push(format!("Conditions: {}.", conditions.join(", ")));
    }
    let mut line = format!("`{}`.", group.metric.replace('`', "'"));
    if !about.is_empty() {
        line.push(' ');
        line.push_str(&about.join(" "));
    }
    let _ = writeln!(out, "{line}\n");
    let cycles = group.rows.iter().any(|row| !row.cycles.is_empty());
    out.push_str("| Terminal | Median | 95 % interval of the median | p95 | Mean ± sd | Min … max | Samples (runs) |");
    out.push_str(if cycles {
        " Each cycle's median |\n"
    } else {
        "\n"
    });
    out.push_str("|---|---|---|---|---|---|---|");
    out.push_str(if cycles { "---|\n" } else { "\n" });
    let digits = Digits::of(group);
    for row in &group.rows {
        let summary = &row.summary;
        let _ = write!(
            out,
            "| {} | {} | {} … {} | {} | {} ± {} | {} … {} | {} ({}) |",
            cell(&row.terminal),
            digits.format(summary.median),
            digits.format(summary.median_interval.0),
            digits.format(summary.median_interval.1),
            digits.format(summary.p95),
            digits.format(summary.mean),
            digits.format(summary.stddev),
            digits.format(summary.min),
            digits.format(summary.max),
            summary.count,
            row.runs,
        );
        if cycles {
            let medians: Vec<String> = row
                .cycles
                .iter()
                .map(|cycle| digits.format(cycle.median))
                .collect();
            let _ = write!(out, " {} |", medians.join(", "));
        }
        out.push('\n');
    }
    for skip in &group.skipped {
        let _ = write!(
            out,
            "| {} | skipped: {} | | | | | |",
            cell(&skip.terminal),
            cell(&skip.why)
        );
        out.push_str(if cycles { " |\n" } else { "\n" });
    }
    out.push('\n');
}

/// A group's feature and telling conditions after its title, as `, text
/// plain`, of `feature` the feature's too.
fn of_conditions(group: &Group, feature: bool) -> String {
    let mut text = String::new();
    if feature && let Some(name) = &group.feature {
        let _ = write!(text, ", of `{}`", name.replace('`', "'"));
    }
    for name in &group.telling {
        if let Some(value) = group.conditions.get(name) {
            let _ = write!(text, ", {} {}", cell(name), cell(&value_text(value)));
        }
    }
    text
}

/// A change as a signed number of `unit`, and its percent if one.
fn change_text(change: f64, percent: Option<f64>, digits: Digits, unit: &str) -> String {
    let mut text = with_unit(&digits.signed(change), unit);
    if let Some(percent) = percent {
        let _ = write!(text, " ({percent:+.1} %)");
    }
    text
}

/// `number` of `unit`, if one.
fn with_unit(number: &str, unit: &str) -> String {
    if unit.is_empty() {
        number.to_owned()
    } else {
        format!("{number} {unit}")
    }
}

/// A condition's value as a text, a text of no quotes.
fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// How many decimals a group's numbers take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Digits {
    /// Whole numbers, as counts: a number between two of a decimal.
    Whole,
    /// This many decimals.
    Fixed(usize),
}

impl Digits {
    /// Those of `group`: whole of whole samples, else of the magnitude of
    /// its greatest median, four figures or so.
    fn of(group: &Group) -> Self {
        let whole = group
            .rows
            .iter()
            .all(|row| row.samples.iter().all(|sample| sample.fract() == 0.0));
        if whole {
            return Self::Whole;
        }
        let greatest = group
            .rows
            .iter()
            .map(|row| row.summary.median.abs())
            .fold(0.0, f64::max);
        Self::Fixed(if greatest >= 100.0 {
            1
        } else if greatest >= 1.0 {
            2
        } else if greatest >= 0.1 {
            3
        } else {
            4
        })
    }

    /// `value` of these decimals.
    fn format(self, value: f64) -> String {
        match self {
            Self::Whole if value.fract() == 0.0 => format!("{value:.0}"),
            Self::Whole => format!("{value:.1}"),
            Self::Fixed(digits) => format!("{value:.digits$}"),
        }
    }

    /// `value` of these decimals and its sign.
    fn signed(self, value: f64) -> String {
        let text = self.format(value);
        if value > 0.0 || (value == 0.0 && !text.starts_with('-')) {
            format!("+{text}")
        } else {
            text
        }
    }
}

/// `text` within a table's cell: no pipe nor line break to break it.
fn cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\n', '\r'], " ")
}

/// The local date and time of `seconds` since 1970, as `2026-11-24 14:05`.
fn local(seconds: u64) -> String {
    let tm = local_tm(seconds);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// The local time of `seconds` since 1970 to the second, as `14:05:31`.
fn local_seconds(seconds: u64) -> String {
    let tm = local_tm(seconds);
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// The local calendar's fields of `seconds` since 1970.
fn local_tm(seconds: u64) -> libc::tm {
    let time = libc::time_t::try_from(seconds).unwrap_or(libc::time_t::MAX);
    // SAFETY: `tm` is plain data, of which zeros are a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to live values of their types, which
    // `localtime_r` reads and writes alone.
    unsafe { libc::localtime_r(&time, &mut tm) };
    tm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{self, Input};
    use serde_json::Map;
    use ttybench_core::budget::{Ab, Budget};
    use ttybench_core::results::{Entry, Machine, Record};

    fn record(tool: &str, terminal: &str, metric: &str, unit: &str, samples: &[f64]) -> Record {
        Record {
            tool: tool.to_owned(),
            terminal: terminal.to_owned(),
            version: Some("1.0".to_owned()),
            metric: metric.to_owned(),
            unit: unit.to_owned(),
            samples: samples.to_vec(),
            conditions: Map::new(),
            cycle: Some(1),
        }
    }

    #[test]
    fn a_report_of_its_budgets_ab_tables_and_runs() {
        let mut throughput = record(
            "throughput",
            "old",
            "written_mib_s",
            "MiB/s",
            &[300.0, 310.0],
        );
        throughput
            .conditions
            .insert("text".into(), "plain|pipe".into());
        let mut smooth = record("scroll", "old", "frames_per_s", "frames/s", &[60.0, 61.5]);
        smooth.conditions.insert("feature".into(), "smooth".into());
        let mut second = record("startup", "old", "ready_ms", "ms", &[121.5, 119.5]);
        second.cycle = Some(2);
        let records = [
            record("startup", "old", "ready_ms", "ms", &[100.5, 110.5]),
            record("startup", "new", "ready_ms", "ms", &[90.5, 95.5]),
            second,
            throughput,
            smooth,
            record("frames", "old", "frames_dropped", "frames", &[1.0, 2.0]),
            record("frames", "new", "frames_dropped", "frames", &[1.0, 2.0]),
        ];
        let log = [Entry {
            id: "c1-r3-scroll-new".to_owned(),
            cycle: 1,
            run: 3,
            position: 1,
            tool: "scroll".to_owned(),
            terminal: "new".to_owned(),
            args: vec!["--feature".to_owned(), "smooth".to_owned()],
            started: 1_700_000_000,
            seconds: 60.0,
            status: Status::Skipped,
            code: Some(3),
            error: Some("new has no feature `smooth`".to_owned()),
            load: None,
        }];
        let budgets: Vec<Budget> = toml_budgets(
            "[[budget]]\nmetric = \"ready_ms\"\nterminal = \"new\"\nat-most = 100\n[[budget]]\nmetric = \"ready_ms\"\nchange-at-most = 0\n[[budget]]\nmetric = \"peak_mib\"\nat-most = 1\n",
        );
        let terminals = ["old".to_owned(), "new".to_owned()];
        let report = analysis::report(&Input {
            session: "2026-11-24-1200".to_owned(),
            machine: Some(Machine {
                model: "Mac16,8".to_owned(),
                cpu: "Apple M4 Pro".to_owned(),
                macos: "27.2".to_owned(),
                refresh_hz: Some(60.0),
            }),
            records: &records,
            log: &log,
            terminals: &terminals,
            ab: Some(Ab {
                a: "old".to_owned(),
                b: "new".to_owned(),
            }),
            budgets: &budgets,
        });
        let text = render(&report);
        for part in [
            "# ttybench session 2026-11-24-1200\n",
            "on Mac16,8 (Apple M4 Pro), macOS 27.2, the main display at 60 Hz. 1 run: 0 done, 0 failed, 1 skipped.",
            "| old | 1.0 |\n| new | 1.0 |\n",
            "## Budgets\n\n2 held, 0 not held, 1 of nothing to judge.\n",
            "| startup · From the launch to running the payload, its median | new | 93.0 ms | ≤ 100 ms | ✔ held |",
            "| startup · From the launch to running the payload, its median | new against old | -22.0 ms (-19.1 %), 95 % ",
            "| ≤ +0 ms | ✔ held |",
            "| peak_mib, its median |  |  | ≤ 1 | no record of it |",
            "## A/B: new against old\n",
            "| frames · Frames dropped (frames) | 1.5 | 1.5 | +0 (+0.0 %) | ",
            "## startup\n\n### From the launch to running the payload (ms)\n\n`ready_ms`.\n",
            "| Each cycle's median |\n",
            "| old | 115.0 | ",
            " | 4 (2) | 105.5, 120.5 |\n",
            "| new | 93.0 | ",
            " | 2 (1) |  |\n",
            "### Its rate to the last write (MiB/s)\n\n`written_mib_s`. Conditions: text plain\\|pipe.\n",
            "### Frames a second that moved the text (frames/s), of `smooth`\n\n`frames_per_s`. Of the feature `smooth`.\n",
            "| new | skipped: new has no feature `smooth` | | | | | |\n",
            " | 1 … 2 | 2 (1) |\n",
        ] {
            assert!(text.contains(part), "{part:?} not in:\n{text}");
        }
        assert!(!text.contains("Runs that measured nothing"), "{text}");
        assert!(text.contains("## Runs\n"), "{text}");
        assert!(text.contains(" | 60 | skipped |  |  |\n"), "{text}");
        assert!(text.ends_with("|\n") && !text.ends_with("\n\n"), "{text}");
    }

    fn toml_budgets(text: &str) -> Vec<Budget> {
        let judged: ttybench_core::budget::Judged =
            ttybench_core::config::parse(std::path::Path::new("s.toml"), text).unwrap();
        judged.budgets
    }

    #[test]
    fn numbers_of_their_groups_magnitude() {
        assert_eq!(Digits::Fixed(1).format(268.14), "268.1");
        assert_eq!(Digits::Fixed(3).format(0.2961), "0.296");
        assert_eq!(Digits::Whole.format(2.0), "2");
        assert_eq!(Digits::Whole.format(2.5), "2.5");
        assert_eq!(Digits::Fixed(2).signed(1.234), "+1.23");
        assert_eq!(Digits::Fixed(2).signed(-1.234), "-1.23");
        assert_eq!(Digits::Fixed(2).signed(0.0), "+0.00");
        assert_eq!(cell("a|b\nc"), "a\\|b c");
    }
}
