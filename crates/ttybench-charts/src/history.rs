//! The history of several sessions: each terminal's metrics along its
//! versions, a chart of each terminal and tool, a panel of each metric,
//! each session's median a dot, in the sessions' order.

use std::fmt::Write;

use serde_json::{Map, Value};

use crate::kinds::Drawn;
use crate::model::{Group, Report};
use crate::svg::{self, Chart, Digits, escape, series, tick_text, ticks};

/// The conditions that tell how a session ran, not what it measured,
/// which may differ between two sessions of the same measure.
const BOOKKEEPING: [&str; 4] = ["rounds", "pause_s", "season", "settle_s"];

/// A panel's room, its plot's and the gap between two panels.
const PANEL_HEIGHT: f64 = 180.0;
const PLOT_TOP: f64 = 52.0;
const PLOT_HEIGHT: f64 = 90.0;
const PLOT_LEFT: f64 = 40.0;
const PLOT_RIGHT: f64 = 52.0;
const GAP: f64 = 28.0;

/// A measure's identity across sessions: its tool, metric, unit, and its
/// conditions but the bookkeeping's.
fn identity(group: &Group) -> String {
    let conditions: Map<String, Value> = group
        .conditions
        .iter()
        .filter(|(name, _)| !BOOKKEEPING.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        group.tool,
        group.metric,
        group.unit,
        Value::Object(conditions)
    )
}

/// A measure of a terminal along the sessions.
struct Line<'a> {
    /// The group of the latest session that has it, for its title.
    group: &'a Group,
    /// Each session's median, if the terminal had the measure there.
    medians: Vec<Option<f64>>,
}

/// The history's charts of `sessions`, the oldest first: a chart of each
/// terminal and tool of two sessions or more.
pub fn history(sessions: &[Report]) -> Vec<Drawn> {
    let mut terminals: Vec<String> = Vec::new();
    for report in sessions {
        for terminal in &report.terminals {
            if !terminals.contains(&terminal.name) {
                terminals.push(terminal.name.clone());
            }
        }
    }
    let mut drawn = Vec::new();
    for (place, terminal) in terminals.iter().enumerate() {
        let present: Vec<bool> = sessions
            .iter()
            .map(|report| {
                report
                    .groups
                    .iter()
                    .any(|group| group.rows.iter().any(|row| &row.terminal == terminal))
            })
            .collect();
        if present.iter().filter(|present| **present).count() < 2 {
            continue;
        }
        let mut tools: Vec<&str> = Vec::new();
        let mut lines: Vec<(String, Line<'_>)> = Vec::new();
        for (at, report) in sessions.iter().enumerate() {
            for group in &report.groups {
                let Some(row) = group.rows.iter().find(|row| &row.terminal == terminal) else {
                    continue;
                };
                let key = identity(group);
                let index = match lines.iter().position(|(other, _)| *other == key) {
                    Some(index) => index,
                    None => {
                        lines.push((
                            key,
                            Line {
                                group,
                                medians: vec![None; sessions.len()],
                            },
                        ));
                        lines.len() - 1
                    }
                };
                lines[index].1.group = group;
                lines[index].1.medians[at] = Some(row.summary.median);
                if !tools.contains(&group.tool.as_str()) {
                    tools.push(&group.tool);
                }
            }
        }
        let labels: Vec<String> = sessions
            .iter()
            .map(|report| {
                report
                    .terminals
                    .iter()
                    .find(|other| &other.name == terminal)
                    .and_then(|other| other.version.clone())
                    .unwrap_or_else(|| report.session.clone())
            })
            .collect();
        for tool in tools {
            let panels: Vec<&Line<'_>> = lines
                .iter()
                .map(|(_, line)| line)
                .filter(|line| line.group.tool == tool)
                .collect();
            drawn.push(chart(terminal, place, tool, &panels, sessions, &labels));
        }
    }
    drawn
}

/// The chart of `terminal`'s measures of `tool`, its `panels`, along
/// `sessions`, labelled of `labels`.
fn chart(
    terminal: &str,
    place: usize,
    tool: &str,
    panels: &[&Line<'_>],
    sessions: &[Report],
    labels: &[String],
) -> Drawn {
    let id = svg::slug(&["history", terminal, tool]);
    let title = format!("{terminal}: {tool} along its versions");
    let mut description = format!(
        "{title}, of {} sessions, {}:",
        sessions.len(),
        sessions
            .iter()
            .zip(labels)
            .map(|(report, label)| format!("{} of {label}", report.session))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for (index, line) in panels.iter().enumerate() {
        let values: Vec<String> = line
            .medians
            .iter()
            .map(|median| median.map_or("none".to_owned(), |median| format!("{median:.3}")))
            .collect();
        let _ = write!(
            description,
            "{} {}, in {}, {}",
            if index == 0 { "" } else { ";" },
            line.group.heading(),
            line.group.unit,
            values.join(", ")
        );
    }
    description.push('.');
    let mut chart = Chart::new(
        &id,
        &title,
        &format!(
            "Each session's median, in the sessions' order, {}",
            sessions
                .iter()
                .map(|report| report.session.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        &description,
    );
    let class = series(place);
    let top = chart.top() + 8.0;
    let width = (svg::width() - 32.0 - GAP) / 2.0;
    for (index, line) in panels.iter().enumerate() {
        let x0 = 16.0 + (index % 2) as f64 * (width + GAP);
        let y0 = top + (index / 2) as f64 * PANEL_HEIGHT;
        panel(&mut chart, line, &class, (x0, y0, width), labels);
    }
    let rows = panels.len().div_ceil(2) as f64;
    let svg = chart.finish(top + rows * PANEL_HEIGHT + 6.0);
    Drawn {
        file: format!("{id}.svg"),
        title,
        kind: "history",
        groups: Vec::new(),
        svg,
    }
}

/// A measure's panel at `(x0, y0)`, `width` wide.
fn panel(
    chart: &mut Chart,
    line: &Line<'_>,
    class: &str,
    (x0, y0, width): (f64, f64, f64),
    labels: &[String],
) {
    let group = line.group;
    let present: Vec<f64> = line.medians.iter().flatten().copied().collect();
    let digits = Digits::of(&present, &present);
    let heading = format!("{} ({})", group.heading(), group.unit);
    let mut lines_of_title = Vec::new();
    let mut current = String::new();
    for word in heading.split(' ') {
        let wider = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if !current.is_empty() && svg::text_width(&wider, 12.0) * 1.1 > width {
            lines_of_title.push(std::mem::take(&mut current));
            current = word.to_owned();
        } else {
            current = wider;
        }
    }
    lines_of_title.push(current);
    for (at, text) in lines_of_title.iter().take(2).enumerate() {
        chart.push(&format!(
            r#"<text class="primary" x="{x0:.1}" y="{:.1}" font-size="12" font-weight="600">{}</text>"#,
            y0 + 14.0 + 15.0 * at as f64,
            escape(text)
        ));
    }
    let highest = present.iter().copied().fold(0.0, f64::max);
    let ticks = ticks(highest, 3);
    let last = *ticks.last().unwrap_or(&1.0);
    let (left, right) = (x0 + PLOT_LEFT, x0 + width - PLOT_RIGHT);
    let bottom = y0 + PLOT_TOP + PLOT_HEIGHT;
    let y_of = |value: f64| bottom - value.max(0.0) / last * PLOT_HEIGHT;
    let count = line.medians.len();
    let x_of = |at: usize| {
        if count < 2 {
            (left + right) / 2.0
        } else {
            left + at as f64 / (count - 1) as f64 * (right - left)
        }
    };
    for tick in &ticks {
        let y = y_of(*tick);
        chart.push(&format!(
            r#"<line class="grid" x1="{left:.1}" y1="{y:.1}" x2="{right:.1}" y2="{y:.1}" stroke-width="1"/>"#
        ));
        chart.push(&format!(
            r#"<text class="muted tick" x="{:.1}" y="{:.1}" font-size="11" text-anchor="end">{}</text>"#,
            left - 6.0,
            y + 4.0,
            tick_text(*tick)
        ));
    }
    // The versions under the plot, each of the room that the sessions
    // leave it, every other one where they crowd.
    let room = (right - left) / count.max(2).saturating_sub(1) as f64;
    // A version of its build goes on two lines, as `1.4.0` over
    // `(202611220721)`.
    let lines_of = |label: &str| -> Vec<String> {
        match label.split_once(" (") {
            Some((version, build)) => vec![version.to_owned(), format!("({build}")],
            None => vec![label.to_owned()],
        }
    };
    let every = if labels.iter().any(|label| {
        lines_of(label)
            .iter()
            .any(|line| svg::text_width(line, 10.0) * 1.1 > room - 4.0)
    }) {
        2
    } else {
        1
    };
    for (at, label) in labels.iter().enumerate() {
        if at % every != 0 && at + 1 != count {
            continue;
        }
        // The first and the last within the plot's ends.
        let anchor = match at {
            0 if count > 1 => "start",
            at if at + 1 == count && count > 1 => "end",
            _ => "middle",
        };
        let x = match anchor {
            "start" => x_of(at) - 4.0,
            "end" => x_of(at) + 4.0,
            _ => x_of(at),
        };
        for (row, line) in lines_of(label).iter().enumerate() {
            chart.push(&format!(
                r#"<text class="muted tick" x="{x:.1}" y="{:.1}" font-size="10" text-anchor="{anchor}">{}</text>"#,
                bottom + 14.0 + 12.0 * row as f64,
                escape(line)
            ));
        }
    }
    // The line through the sessions that have the measure, then a dot of
    // each.
    let mut path = String::new();
    for (at, median) in line.medians.iter().enumerate() {
        if let Some(median) = median {
            let _ = write!(
                path,
                "{}{:.1},{:.1} ",
                if path.is_empty() { "M" } else { "L" },
                x_of(at),
                y_of(*median)
            );
        }
    }
    if present.len() > 1 {
        chart.push(&format!(
            r#"<path class="{class}-line" d="{}" fill="none" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>"#,
            path.trim_end()
        ));
    }
    let lastly = line.medians.iter().rposition(Option::is_some);
    for (at, median) in line.medians.iter().enumerate() {
        let Some(median) = median else {
            continue;
        };
        let (x, y) = (x_of(at), y_of(*median));
        chart.push(&format!(
            r#"<circle class="{class} ring" cx="{x:.1}" cy="{y:.1}" r="4.5" stroke-width="2"><title>{}</title></circle>"#,
            escape(&format!(
                "{}: {} {}",
                labels[at],
                digits.format(*median),
                group.unit
            ))
        ));
        if Some(at) == lastly {
            chart.push(&format!(
                r#"<text class="primary tick" x="{:.1}" y="{:.1}" font-size="11" font-weight="600">{}</text>"#,
                x + 8.0,
                y + 4.0,
                digits.format(*median)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(
        name: &str,
        versions: &[(&str, &str)],
        medians: &[(&str, f64)],
        rounds: u32,
    ) -> Report {
        let rows: Vec<Value> = medians
            .iter()
            .map(|(terminal, median)| {
                json!({"terminal": terminal, "summary": {
                    "count": 3, "min": median, "median": median, "p95": median, "p99": median,
                    "max": median, "mean": median, "stddev": 0.0,
                    "median_interval": [median, median]}})
            })
            .collect();
        serde_json::from_value(json!({
            "session": name,
            "terminals": versions.iter().map(|(name, version)| json!({"name": name, "version": version})).collect::<Vec<_>>(),
            "groups": [
                {"tool": "startup", "metric": "ready_ms", "unit": "ms", "title": "Ready",
                 "conditions": {"rounds": rounds, "cols": 120}, "rows": rows},
                {"tool": "throughput", "metric": "written_mib_s", "unit": "MiB/s", "title": "Rate",
                 "conditions": {"text": "plain"}, "rows": [rows[0].clone()]}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn each_terminal_of_two_sessions_along_its_versions() {
        let sessions = [
            session(
                "one",
                &[("lokyr", "1.4.0"), ("ghostty", "1.4.0 (18457)")],
                &[("lokyr", 250.0), ("ghostty", 260.0)],
                3,
            ),
            session(
                "two",
                &[("lokyr", "1.5.0"), ("kitty", "0.51.0")],
                &[("lokyr", 240.0), ("kitty", 600.0)],
                5,
            ),
        ];
        let drawn = history(&sessions);
        let files: Vec<&str> = drawn.iter().map(|chart| chart.file.as_str()).collect();
        // Ghostty and kitty are of one session each: no history.
        assert_eq!(
            files,
            ["history-lokyr-startup.svg", "history-lokyr-throughput.svg"]
        );
        let startup = &drawn[0].svg;
        // The rounds tell how the sessions ran: one line through both.
        assert!(
            startup.contains("Ready, in ms, 250.000, 240.000"),
            "{startup}"
        );
        assert!(startup.contains(">1.4.0</text>") && startup.contains(">1.5.0</text>"));
        // Ghostty's build under its version, in its own history.
        assert!(!startup.contains("(18457)"));
        assert!(startup.contains("<title>1.5.0: 240 ms</title>"));
        assert!(startup.contains(r#"<path class="t0-line""#));
        // The throughput of the first terminal of each session: lokyr's.
        assert!(drawn[1].svg.contains("Rate, in MiB/s, 250.000, 240.000"));
    }
}
