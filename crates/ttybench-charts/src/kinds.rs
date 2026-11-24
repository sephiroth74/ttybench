//! The charts of a session, each of its kind: bars of each terminal's
//! median and its 95th percentile; the latency's distribution, whole and
//! of its tail; the frames' intervals in time; the GPU's time of the
//! terminal on WindowServer's; and the A/B's change of each metric.

use std::fmt::Write;

use serde::Serialize;
use ttybench_core::budget::{Budget, Change as Limit};
use ttybench_core::results::Record;

use crate::model::{Group, Report, Verdict};
use crate::svg::{self, Chart, Digits, WIDTH, bar_path, escape, series, tick_text, ticks};

/// A chart drawn: its file's name, its title, its kind, the groups that
/// it shows, by their place in the report's, and its SVG.
#[derive(Clone, Debug, Serialize)]
pub struct Drawn {
    /// Its file's name, in the charts' folder.
    pub file: String,
    /// Its title.
    pub title: String,
    /// Its kind: `bars`, `distribution`, `intervals`, `gpu` or `ab`.
    pub kind: &'static str,
    /// The groups that it shows.
    pub groups: Vec<usize>,
    /// Its SVG.
    #[serde(skip)]
    pub svg: String,
}

/// The metric whose distribution a chart shows whole.
const DISTRIBUTION: (&str, &str) = ("latency", "latency_ms");

/// The metrics of the gaps between frames, which a chart shows in time.
const INTERVALS: [(&str, &str); 2] = [("scroll", "frame_gap_ms"), ("frames", "frame_gap_ms")];

/// The GPU's metrics of the terminal's own time and of WindowServer's,
/// which a chart stacks.
const GPU: (&str, &str, &str) = ("gpu", "terminal_gpu_ms_per_s", "window_server_gpu_ms_per_s");

/// Where the bars start, after the terminals' names.
const LEFT: f64 = 118.0;

/// The room after the bars, of their values.
const RIGHT: f64 = 128.0;

/// A row's band, and a bar's thickness.
const BAND: f64 = 30.0;
const BAR: f64 = 18.0;

/// The room under a plot, of its axis's ticks.
const AXIS: f64 = 34.0;

/// Every chart of `report`, of the samples of `records`: the A/B's first,
/// then each group's of its kind, in the report's order.
pub fn all(report: &Report, records: &[Record]) -> Vec<Drawn> {
    let mut drawn = Vec::new();
    if let Some(chart) = ab(report) {
        drawn.push(chart);
    }
    for (at, group) in report.groups.iter().enumerate() {
        let key = (group.tool.as_str(), group.metric.as_str());
        let chart = if key == DISTRIBUTION {
            distribution(report, at, records)
        } else if INTERVALS.contains(&key) {
            intervals(report, at, records)
        } else if key == (GPU.0, GPU.1) {
            gpu(report, at).or_else(|| bars(report, at, records))
        } else if key == (GPU.0, GPU.2)
            && pair_of(report, at).is_some_and(|own| gpu(report, own).is_some())
        {
            // Stacked after the terminal's own.
            None
        } else {
            bars(report, at, records)
        };
        drawn.extend(chart);
    }
    // Each file's name once.
    let mut seen: Vec<String> = Vec::new();
    for chart in &mut drawn {
        let stem = chart.file.trim_end_matches(".svg").to_owned();
        let mut file = chart.file.clone();
        let mut count = 1;
        while seen.contains(&file) {
            count += 1;
            file = format!("{stem}-{count}.svg");
        }
        if file != chart.file {
            for part in ["title", "desc"] {
                chart.svg = chart
                    .svg
                    .replace(&format!("{stem}-{part}"), &format!("{stem}-{count}-{part}"));
            }
            chart.file = file.clone();
        }
        seen.push(file);
    }
    drawn
}

/// A group's file's stem: its tool, its metric, its feature and its
/// telling conditions.
fn stem(group: &Group) -> String {
    let mut parts = vec![group.tool.clone(), group.metric.clone()];
    parts.extend(group.feature.clone());
    for name in &group.telling {
        if let Some(value) = group.conditions.get(name) {
            parts.push(format!("{name}-{}", crate::model::value_text(value)));
        }
    }
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    svg::slug(&parts)
}

/// A group's digits, of its samples and medians.
fn digits_of(group: &Group, records: &[Record]) -> Digits {
    let samples: Vec<f64> = group
        .rows
        .iter()
        .flat_map(|row| group.samples(&row.terminal, records))
        .collect();
    let medians: Vec<f64> = group.rows.iter().map(|row| row.summary.median).collect();
    Digits::of(&samples, &medians)
}

/// The budgets that judged the terminal `terminal` of the group at `at`.
fn budgets_of<'a>(report: &'a Report, at: usize, terminal: Option<&str>) -> Vec<&'a Budget> {
    let mut budgets = Vec::new();
    for judgement in &report.judgements {
        if judgement.group == Some(at)
            && judgement.terminal.as_deref() == terminal
            && let Some(budget) = report.budgets.get(judgement.budget.wrapping_sub(1))
            && !budgets.contains(&budget)
        {
            budgets.push(budget);
        }
    }
    budgets
}

/// Whether a budget held, of the terminal `terminal` of the group at `at`.
fn held(report: &Report, at: usize, terminal: Option<&str>, budget: &Budget) -> Option<bool> {
    report
        .judgements
        .iter()
        .find(|judgement| {
            judgement.group == Some(at)
                && judgement.terminal.as_deref() == terminal
                && report.budgets.get(judgement.budget.wrapping_sub(1)) == Some(budget)
        })
        .and_then(|judgement| judgement.held)
}

/// `held`'s words.
fn held_words(held: Option<bool>) -> &'static str {
    match held {
        Some(true) => "held",
        Some(false) => "not held",
        None => "not judged",
    }
}

/// The vertical grid of `ticks` from `top` to `bottom`, of `scale` from
/// `left`, their texts under it, `suffix` after each, and the baseline.
fn vertical_axis(
    chart: &mut Chart,
    ticks: &[f64],
    scale: &dyn Fn(f64) -> f64,
    (top, bottom): (f64, f64),
    left: f64,
    suffix: &str,
) {
    for tick in ticks {
        let x = scale(*tick);
        chart.push(&format!(
            r#"<line class="grid" x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{bottom:.1}" stroke-width="1"/>"#,
            top - 6.0
        ));
        let text = if *tick > 0.0 && suffix == " %" && left < 0.0 {
            format!("+{}{suffix}", tick_text(*tick))
        } else if *tick == 0.0 {
            "0".to_owned()
        } else {
            format!("{}{suffix}", tick_text(*tick))
        };
        chart.push(&format!(
            r#"<text class="muted tick" x="{x:.1}" y="{:.1}" font-size="11" text-anchor="middle">{}</text>"#,
            bottom + 15.0,
            escape(&text)
        ));
    }
    if left >= 0.0 {
        chart.push(&format!(
            r#"<line class="base" x1="{left:.1}" y1="{:.1}" x2="{left:.1}" y2="{bottom:.1}" stroke-width="1"/>"#,
            top - 6.0
        ));
    }
}

/// A legend's item: its mark, drawn about a point, and its text.
type Item<'a> = (&'a dyn Fn(f64, f64) -> String, String);

/// A legend's items from `x` at `y`: each a mark's SVG at its left, of
/// `(mark, text)`, the mark drawn about `(x, y)`.
fn legend(chart: &mut Chart, y: f64, items: &[Item<'_>]) {
    let mut x = 22.0;
    for (mark, text) in items {
        chart.push(&mark(x, y));
        chart.push(&format!(
            r#"<text class="secondary" x="{:.1}" y="{:.1}" font-size="12">{}</text>"#,
            x + 12.0,
            y + 4.0,
            escape(text)
        ));
        x += 12.0 + svg::text_width(text, 12.0) + 22.0;
    }
}

/// A budget's tick in a legend.
fn budget_mark(x: f64, y: f64) -> String {
    format!(
        r#"<line class="budget" x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{:.1}" stroke-width="2"/>"#,
        y - 7.0,
        y + 7.0
    )
}

/// Bars of each terminal's median of the group at `at`, a ring of its
/// 95th percentile, and a tick of each budget that judged it.
fn bars(report: &Report, at: usize, records: &[Record]) -> Option<Drawn> {
    let group = &report.groups[at];
    if group.rows.is_empty() || group.rows.iter().any(|row| row.summary.min < 0.0) {
        return None;
    }
    let digits = digits_of(group, records);
    let unit = &group.unit;
    let heading = group.heading();
    let mut description = format!("{heading}, in {unit}:");
    for (index, row) in group.rows.iter().enumerate() {
        let summary = &row.summary;
        let _ = write!(
            description,
            "{} {}, median {}, 95th percentile {}, of {} samples",
            if index == 0 { "" } else { ";" },
            row.terminal,
            digits.format(summary.median),
            digits.format(summary.p95),
            summary.count
        );
    }
    description.push('.');
    let id = stem(group);
    let mut chart = Chart::new(
        &id,
        &heading,
        &if group
            .rows
            .iter()
            .any(|row| row.summary.count > 1 && row.summary.p95 > row.summary.median)
        {
            format!(
                "{} · each terminal's median, a bar, and its 95th percentile, a ring, in {unit}",
                group.tool
            )
        } else {
            format!("{} · each terminal's median, in {unit}", group.tool)
        },
        &description,
    );
    let legend_y = chart.top() + 2.0;
    let top = chart.top() + 22.0;
    let bottom = top + BAND * group.rows.len() as f64 - 6.0;
    let limits: Vec<Vec<(f64, &Budget)>> = group
        .rows
        .iter()
        .map(|row| {
            budgets_of(report, at, Some(&row.terminal))
                .into_iter()
                .flat_map(|budget| {
                    budget
                        .at_most
                        .into_iter()
                        .chain(budget.at_least)
                        .map(move |limit| (limit, budget))
                })
                .collect()
        })
        .collect();
    let highest = group
        .rows
        .iter()
        .map(|row| row.summary.p95.max(row.summary.median))
        .chain(limits.iter().flatten().map(|(limit, _)| *limit))
        .fold(0.0, f64::max);
    // A count's ticks are whole.
    let ticks = if digits == Digits::Whole && highest <= 5.0 {
        (0..=(highest.ceil() as u32).max(1))
            .map(f64::from)
            .collect()
    } else {
        ticks(highest, 5)
    };
    let span = WIDTH - LEFT - RIGHT;
    let last = *ticks.last().unwrap_or(&1.0);
    let scale = |value: f64| LEFT + value.max(0.0) / last * span;
    let rings = group
        .rows
        .iter()
        .any(|row| row.summary.count > 1 && row.summary.p95 > row.summary.median);
    let mut items: Vec<Item<'_>> = vec![(
        &|x: f64, y: f64| {
            format!(
                r#"<rect class="context" x="{:.1}" y="{:.1}" width="12" height="10" rx="2"/>"#,
                x - 6.0,
                y - 5.0
            )
        },
        "median".to_owned(),
    )];
    if rings {
        items.push((
            &|x: f64, y: f64| {
                format!(
                    r#"<circle class="surface context-line" cx="{x:.1}" cy="{y:.1}" r="4.5" stroke-width="2"/>"#
                )
            },
            "95th percentile".to_owned(),
        ));
    }
    if limits.iter().any(|row| !row.is_empty()) {
        items.push((&budget_mark, "budget".to_owned()));
    }
    legend(&mut chart, legend_y, &items);
    vertical_axis(&mut chart, &ticks, &scale, (top, bottom), LEFT, "");
    for (index, row) in group.rows.iter().enumerate() {
        let summary = &row.summary;
        let class = series(report.place(&row.terminal));
        let y = top + index as f64 * BAND + (BAND - BAR) / 2.0 - 6.0;
        let middle = y + BAR / 2.0;
        let mut hit = format!(
            "{}: median {} {unit}, 95th percentile {} {unit}, of {} samples",
            row.terminal,
            digits.format(summary.median),
            digits.format(summary.p95),
            summary.count
        );
        chart.push(&format!(
            r#"<text class="secondary" x="{:.1}" y="{:.1}" font-size="12" text-anchor="end">{}</text>"#,
            LEFT - 10.0,
            middle + 4.0,
            escape(&row.terminal)
        ));
        chart.push(&format!(
            r#"<path class="{class}" d="{}"/>"#,
            bar_path(LEFT, y, (scale(summary.median) - LEFT).max(1.0), BAR)
        ));
        let mut end = scale(summary.median);
        if summary.count > 1 && summary.p95 > summary.median {
            let x = scale(summary.p95);
            chart.push(&format!(
                r#"<line class="{class}-line" x1="{end:.1}" y1="{middle:.1}" x2="{x:.1}" y2="{middle:.1}" stroke-width="2" stroke-linecap="round"/>"#
            ));
            chart.push(&format!(
                r#"<circle class="surface {class}-line" cx="{x:.1}" cy="{middle:.1}" r="4.5" stroke-width="2"/>"#
            ));
            end = x + 4.5;
        }
        for (limit, budget) in &limits[index] {
            let x = scale(*limit);
            let words = held_words(held(report, at, Some(&row.terminal), budget));
            let _ = write!(hit, "; budget {} {}", budget.describe_limits(unit), words);
            chart.push(&format!(
                r#"<line class="budget" x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{:.1}" stroke-width="2"/>"#,
                y - 4.0,
                y + BAR + 4.0
            ));
            end = end.max(x + 1.0);
        }
        let p95 = if summary.count > 1 {
            format!(
                r#"<tspan class="secondary" font-weight="400"> · p95 {}</tspan>"#,
                digits.format(summary.p95)
            )
        } else {
            String::new()
        };
        chart.push(&format!(
            r#"<text class="primary tick" x="{:.1}" y="{:.1}" font-size="12" font-weight="600">{}{p95}</text>"#,
            end + 8.0,
            middle + 4.0,
            digits.format(summary.median)
        ));
        chart.push(&format!(
            r#"<rect class="hit" x="0" y="{:.1}" width="{WIDTH}" height="{BAND}"><title>{}</title></rect>"#,
            y - (BAND - BAR) / 2.0,
            escape(&hit)
        ));
    }
    let height = bottom + 6.0 + AXIS;
    let svg = chart.finish(height);
    Some(Drawn {
        file: format!("{id}.svg"),
        title: heading,
        kind: "bars",
        groups: vec![at],
        svg,
    })
}

/// The plots' margins of the charts of a time or a share on their x axis.
const PLOT_LEFT: f64 = 56.0;
const PLOT_RIGHT: f64 = 28.0;

/// The legend of each terminal of `group`, a short line of its color
/// beside its name.
fn terminals_legend(chart: &mut Chart, report: &Report, group: &Group, y: f64) {
    let mut x = 16.0;
    for row in &group.rows {
        let class = series(report.place(&row.terminal));
        chart.push(&format!(
            r#"<line class="{class}-line" x1="{x:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke-width="2" stroke-linecap="round"/>"#,
            x + 16.0
        ));
        chart.push(&format!(
            r#"<text class="secondary" x="{:.1}" y="{:.1}" font-size="12">{}</text>"#,
            x + 22.0,
            y + 4.0,
            escape(&row.terminal)
        ));
        x += 22.0 + svg::text_width(&row.terminal, 12.0) + 22.0;
    }
}

/// The latency's distribution of the group at `at`: the share of the
/// samples at or under each time, whole, then its slowest tenth.
fn distribution(report: &Report, at: usize, records: &[Record]) -> Option<Drawn> {
    let group = &report.groups[at];
    let sorted: Vec<(usize, &str, Vec<f64>)> = group
        .rows
        .iter()
        .map(|row| {
            let mut samples = group.samples(&row.terminal, records);
            samples.sort_by(f64::total_cmp);
            (report.place(&row.terminal), row.terminal.as_str(), samples)
        })
        .filter(|(_, _, samples)| !samples.is_empty())
        .collect();
    if sorted.is_empty() {
        return bars(report, at, records);
    }
    let unit = &group.unit;
    let heading = group.heading();
    let digits = digits_of(group, records);
    let mut description = format!(
        "The distribution of {}, in {unit}, of each terminal:",
        heading.to_lowercase()
    );
    for (index, row) in group.rows.iter().enumerate() {
        let summary = &row.summary;
        let _ = write!(
            description,
            "{} {}, median {}, 95th percentile {}, 99th {}, the slowest {}",
            if index == 0 { "" } else { ";" },
            row.terminal,
            digits.format(summary.median),
            digits.format(summary.p95),
            digits.format(summary.p99),
            digits.format(summary.max)
        );
    }
    description.push('.');
    let id = stem(group);
    let mut chart = Chart::new(
        &id,
        &heading,
        &format!(
            "{} · the share of the samples at or under each time, in {unit}, then the slowest tenth",
            group.tool
        ),
        &description,
    );
    let legend_y = chart.top() + 2.0;
    terminals_legend(&mut chart, report, group, legend_y);
    let whole = (chart.top() + 44.0, 170.0);
    let tail = (whole.0 + whole.1 + 44.0, 140.0);
    let highest = sorted
        .iter()
        .filter_map(|(_, _, samples)| samples.last().copied())
        .fold(0.0, f64::max);
    let ticks = ticks(highest, 6);
    let last = *ticks.last().unwrap_or(&1.0);
    let span = WIDTH - PLOT_LEFT - PLOT_RIGHT;
    let x_of = |value: f64| PLOT_LEFT + value.max(0.0) / last * span;
    for ((top, height), shares, title) in [
        (whole, &[0.0, 0.25, 0.5, 0.75, 1.0][..], "Every sample"),
        (tail, &[0.9, 0.95, 0.99, 1.0][..], "The slowest tenth"),
    ] {
        let low = shares[0];
        let y_of = |share: f64| top + height - (share.max(low) - low) / (1.0 - low) * height;
        chart.push(&format!(
            r#"<text class="secondary" x="{PLOT_LEFT:.1}" y="{:.1}" font-size="12" font-weight="600">{title}</text>"#,
            top - 12.0
        ));
        for share in shares {
            let y = y_of(*share);
            chart.push(&format!(
                r#"<line class="grid" x1="{PLOT_LEFT:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke-width="1"/>"#,
                WIDTH - PLOT_RIGHT
            ));
            chart.push(&format!(
                r#"<text class="muted tick" x="{:.1}" y="{:.1}" font-size="11" text-anchor="end">{} %</text>"#,
                PLOT_LEFT - 8.0,
                y + 4.0,
                tick_text(share * 100.0)
            ));
        }
        for tick in &ticks {
            let x = x_of(*tick);
            chart.push(&format!(
                r#"<line class="grid" x1="{x:.1}" y1="{top:.1}" x2="{x:.1}" y2="{:.1}" stroke-width="1"/>"#,
                top + height
            ));
        }
        for (place, terminal, samples) in &sorted {
            let class = series(*place);
            let count = samples.len() as f64;
            let first = samples
                .iter()
                .enumerate()
                .position(|(index, _)| (index + 1) as f64 / count >= low)
                .unwrap_or(0);
            let mut path = format!(
                "M{:.1},{:.1}",
                x_of(samples[first]),
                y_of((first as f64 / count).max(low))
            );
            for (index, sample) in samples.iter().enumerate().skip(first) {
                let _ = write!(
                    path,
                    " H{:.1} V{:.1}",
                    x_of(*sample),
                    y_of((index + 1) as f64 / count)
                );
            }
            chart.push(&format!(
                r#"<path class="{class}-line" d="{path}" fill="none" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"><title>{}</title></path>"#,
                escape(&format!("{terminal}, {} samples", samples.len()))
            ));
        }
    }
    let bottom = tail.0 + tail.1;
    for tick in &ticks {
        chart.push(&format!(
            r#"<text class="muted tick" x="{:.1}" y="{:.1}" font-size="11" text-anchor="middle">{}</text>"#,
            x_of(*tick),
            bottom + 15.0,
            tick_text(*tick)
        ));
    }
    chart.push(&format!(
        r#"<text class="muted" x="{:.1}" y="{:.1}" font-size="11" text-anchor="end">{}</text>"#,
        WIDTH - PLOT_RIGHT,
        bottom + 30.0,
        escape(unit)
    ));
    let svg = chart.finish(bottom + AXIS + 6.0);
    Some(Drawn {
        file: format!("{id}.svg"),
        title: heading,
        kind: "distribution",
        groups: vec![at],
        svg,
    })
}

/// The gaps between frames of the group at `at` in time, a panel of each
/// terminal, and a line of a refresh's length where the conditions tell
/// the display's rate.
fn intervals(report: &Report, at: usize, records: &[Record]) -> Option<Drawn> {
    let group = &report.groups[at];
    let series_of: Vec<(usize, &str, Vec<f64>)> = group
        .rows
        .iter()
        .map(|row| {
            (
                report.place(&row.terminal),
                row.terminal.as_str(),
                group.samples(&row.terminal, records),
            )
        })
        .filter(|(_, _, samples)| samples.len() > 1)
        .collect();
    if series_of.is_empty() {
        return bars(report, at, records);
    }
    let unit = &group.unit;
    let heading = format!("{} in time", group.heading());
    let digits = digits_of(group, records);
    let refresh = group.refresh_hz().map(|hz| 1000.0 / hz);
    let mut description = format!("{heading}, in {unit}, a panel of each terminal:");
    for (index, (_, terminal, samples)) in series_of.iter().enumerate() {
        let summary = ttybench_core::stats::summary(samples)?;
        let _ = write!(
            description,
            "{} {terminal}, {} frames over {:.1} s, median {}, 95th percentile {}, the longest {}",
            if index == 0 { "" } else { ";" },
            samples.len() + 1,
            samples.iter().sum::<f64>() / 1000.0,
            digits.format(summary.median),
            digits.format(summary.p95),
            digits.format(summary.max)
        );
    }
    if let Some(refresh) = refresh {
        let _ = write!(description, "; a refresh lasts {refresh:.1} ms");
    }
    description.push('.');
    let id = format!("{}-time", stem(group));
    let mut chart = Chart::new(
        &id,
        &heading,
        &format!(
            "{} · the gap before each frame, in {unit}, along the time of the measure, in seconds",
            group.tool
        ),
        &description,
    );
    let highest = series_of
        .iter()
        .flat_map(|(_, _, samples)| samples.iter().copied())
        .chain(refresh)
        .fold(0.0, f64::max);
    let y_ticks = ticks(highest, 3);
    let y_last = *y_ticks.last().unwrap_or(&1.0);
    let longest = series_of
        .iter()
        .map(|(_, _, samples)| samples.iter().sum::<f64>() / 1000.0)
        .fold(0.0, f64::max);
    let x_ticks = ticks(longest, 6);
    let x_last = *x_ticks.last().unwrap_or(&1.0);
    let span = WIDTH - PLOT_LEFT - PLOT_RIGHT;
    let x_of = |seconds: f64| PLOT_LEFT + seconds / x_last * span;
    const PANEL: f64 = 90.0;
    const GAP: f64 = 34.0;
    let mut top = chart.top() + 26.0;
    for (place, terminal, samples) in &series_of {
        let class = series(*place);
        let y_of = |value: f64| top + PANEL - value.max(0.0) / y_last * PANEL;
        chart.push(&format!(
            r#"<line class="{class}-line" x1="{PLOT_LEFT:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke-width="2" stroke-linecap="round"/>"#,
            top - 12.0,
            PLOT_LEFT + 16.0,
            top - 12.0
        ));
        chart.push(&format!(
            r#"<text class="secondary" x="{:.1}" y="{:.1}" font-size="12" font-weight="600">{}</text>"#,
            PLOT_LEFT + 22.0,
            top - 8.0,
            escape(terminal)
        ));
        for tick in &y_ticks {
            let y = y_of(*tick);
            chart.push(&format!(
                r#"<line class="grid" x1="{PLOT_LEFT:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke-width="1"/>"#,
                WIDTH - PLOT_RIGHT
            ));
            chart.push(&format!(
                r#"<text class="muted tick" x="{:.1}" y="{:.1}" font-size="11" text-anchor="end">{}</text>"#,
                PLOT_LEFT - 8.0,
                y + 4.0,
                tick_text(*tick)
            ));
        }
        if let Some(refresh) = refresh {
            let y = y_of(refresh);
            chart.push(&format!(
                r#"<line class="base" x1="{PLOT_LEFT:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke-width="1"/>"#,
                WIDTH - PLOT_RIGHT
            ));
            chart.push(&format!(
                r#"<text class="muted" x="{:.1}" y="{:.1}" font-size="11" text-anchor="end">a refresh, {refresh:.1} {}</text>"#,
                WIDTH - PLOT_RIGHT,
                y - 4.0,
                escape(unit)
            ));
        }
        let mut elapsed = 0.0;
        let mut points = String::new();
        for gap in samples {
            elapsed += gap / 1000.0;
            let _ = write!(points, "{:.1},{:.1} ", x_of(elapsed), y_of(*gap));
        }
        let summary = ttybench_core::stats::summary(samples)?;
        chart.push(&format!(
            r#"<polyline class="{class}-line" points="{}" fill="none" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round"><title>{}</title></polyline>"#,
            points.trim_end(),
            escape(&format!(
                "{terminal}: median {} {unit}, 95th percentile {}, the longest {}",
                digits.format(summary.median),
                digits.format(summary.p95),
                digits.format(summary.max)
            ))
        ));
        top += PANEL + GAP;
    }
    let bottom = top - GAP;
    for tick in &x_ticks {
        chart.push(&format!(
            r#"<text class="muted tick" x="{:.1}" y="{:.1}" font-size="11" text-anchor="middle">{} s</text>"#,
            x_of(*tick),
            bottom + 15.0,
            tick_text(*tick)
        ));
    }
    let svg = chart.finish(bottom + AXIS);
    Some(Drawn {
        file: format!("{id}.svg"),
        title: heading,
        kind: "intervals",
        groups: vec![at],
        svg,
    })
}

/// The group of WindowServer's GPU time of the same conditions as the
/// terminal's own of the group at `at`, if one.
fn pair_of(report: &Report, at: usize) -> Option<usize> {
    let group = &report.groups[at];
    let other = if group.metric == GPU.1 { GPU.2 } else { GPU.1 };
    report.groups.iter().position(|candidate| {
        candidate.tool == group.tool
            && candidate.metric == other
            && candidate.conditions == group.conditions
    })
}

/// The GPU's time of each terminal of the group at `at`, its own then
/// WindowServer's, stacked.
fn gpu(report: &Report, at: usize) -> Option<Drawn> {
    let own = &report.groups[at];
    let server = &report.groups[pair_of(report, at)?];
    let rows: Vec<(usize, &str, f64, f64)> = own
        .rows
        .iter()
        .filter_map(|row| {
            let other = server
                .rows
                .iter()
                .find(|other| other.terminal == row.terminal)?;
            Some((
                report.place(&row.terminal),
                row.terminal.as_str(),
                row.summary.median.max(0.0),
                other.summary.median.max(0.0),
            ))
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    let unit = &own.unit;
    let qualifier = own.qualifier();
    let heading = if qualifier.is_empty() {
        "The GPU's time".to_owned()
    } else {
        format!("The GPU's time, {qualifier}")
    };
    let digits = Digits::of(
        &[],
        &rows
            .iter()
            .map(|(_, _, own, server)| own + server)
            .collect::<Vec<f64>>(),
    );
    let mut description = format!("{heading}, in {unit}, the median of each:");
    for (index, (_, terminal, mine, theirs)) in rows.iter().enumerate() {
        let _ = write!(
            description,
            "{} {terminal}, its own {}, WindowServer's {}",
            if index == 0 { "" } else { ";" },
            digits.format(*mine),
            digits.format(*theirs)
        );
    }
    description.push('.');
    let id = format!("{}-stacked", stem(own));
    let mut chart = Chart::new(
        &id,
        &heading,
        &format!(
            "{} · each terminal's own GPU time, in its color, then WindowServer's, which composes its windows, in gray; medians, in {unit}",
            own.tool
        ),
        &description,
    );
    let legend_y = chart.top() + 2.0;
    legend(
        &mut chart,
        legend_y,
        &[(
            &|x: f64, y: f64| {
                format!(
                    r#"<rect class="context" x="{:.1}" y="{:.1}" width="12" height="10" rx="2"/>"#,
                    x - 6.0,
                    y - 5.0
                )
            },
            "WindowServer".to_owned(),
        )],
    );
    let top = chart.top() + 22.0;
    let bottom = top + BAND * rows.len() as f64 - 6.0;
    let highest = rows
        .iter()
        .map(|(_, _, mine, theirs)| mine + theirs)
        .fold(0.0, f64::max);
    let ticks = ticks(highest, 5);
    let last = *ticks.last().unwrap_or(&1.0);
    let span = WIDTH - LEFT - RIGHT;
    let scale = |value: f64| LEFT + value / last * span;
    vertical_axis(&mut chart, &ticks, &scale, (top, bottom), LEFT, "");
    for (index, (place, terminal, mine, theirs)) in rows.iter().enumerate() {
        let class = series(*place);
        let y = top + index as f64 * BAND + (BAND - BAR) / 2.0 - 6.0;
        let middle = y + BAR / 2.0;
        chart.push(&format!(
            r#"<text class="secondary" x="{:.1}" y="{:.1}" font-size="12" text-anchor="end">{}</text>"#,
            LEFT - 10.0,
            middle + 4.0,
            escape(terminal)
        ));
        let own_end = scale(*mine);
        chart.push(&format!(
            r#"<rect class="{class}" x="{LEFT:.1}" y="{y:.1}" width="{:.1}" height="{BAR}"/>"#,
            (own_end - LEFT).max(1.0)
        ));
        // A gap of the surface between the two.
        let start = own_end.max(LEFT + 1.0) + 2.0;
        let length = (scale(mine + theirs) - start).max(1.0);
        chart.push(&format!(
            r#"<path class="context" d="{}"/>"#,
            bar_path(start, y, length, BAR)
        ));
        chart.push(&format!(
            r#"<text class="primary tick" x="{:.1}" y="{:.1}" font-size="12" font-weight="600">{}<tspan class="secondary" font-weight="400"> + {}</tspan></text>"#,
            start + length + 8.0,
            middle + 4.0,
            digits.format(*mine),
            digits.format(*theirs)
        ));
        chart.push(&format!(
            r#"<rect class="hit" x="0" y="{:.1}" width="{WIDTH}" height="{BAND}"><title>{}</title></rect>"#,
            y - (BAND - BAR) / 2.0,
            escape(&format!(
                "{terminal}: its own {} {unit}, WindowServer's {} {unit}",
                digits.format(*mine),
                digits.format(*theirs)
            ))
        ));
    }
    let svg = chart.finish(bottom + 6.0 + AXIS);
    Some(Drawn {
        file: format!("{id}.svg"),
        title: heading,
        kind: "gpu",
        groups: vec![at, pair_of(report, at)?],
        svg,
    })
}

/// A change's limit in percent of A's value `a`, if it can be.
fn limit_percent(limit: Limit, a: f64) -> Option<f64> {
    match limit {
        Limit::Percent(percent) => Some(percent),
        Limit::Units(units) => (a != 0.0).then(|| units / a.abs() * 100.0),
    }
}

/// The A/B's change of each metric, in percent of A's median, its interval
/// and the budgets of its change.
fn ab(report: &Report) -> Option<Drawn> {
    let ab = report.ab.as_ref()?;
    let rows: Vec<(usize, &Group, crate::model::Change, f64)> = report
        .groups
        .iter()
        .enumerate()
        .filter_map(|(at, group)| {
            let change = group.change?;
            Some((at, group, change, change.percent?))
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    let percent_of = |value: f64, a: f64| value / a.abs() * 100.0;
    let limits: Vec<Vec<(f64, &Budget)>> = rows
        .iter()
        .map(|(at, _, change, _)| {
            budgets_of(report, *at, None)
                .into_iter()
                .flat_map(|budget| {
                    budget
                        .change_at_most
                        .into_iter()
                        .chain(budget.change_at_least)
                        .filter_map(move |limit| {
                            limit_percent(limit, change.a).map(|percent| (percent, budget))
                        })
                })
                .collect()
        })
        .collect();
    let extent = rows
        .iter()
        .flat_map(|(_, _, change, percent)| {
            [
                percent.abs(),
                percent_of(change.interval.0, change.a).abs(),
                percent_of(change.interval.1, change.a).abs(),
            ]
        })
        .chain(limits.iter().flatten().map(|(limit, _)| limit.abs()))
        .filter(|value| value.is_finite())
        .fold(0.0, f64::max);
    let half = ticks(extent, 3);
    let last = *half.last().unwrap_or(&1.0);
    let mut both: Vec<f64> = half.iter().rev().map(|tick| -tick).collect();
    both.extend(half.iter().skip(1));
    const RIGHT_ROOM: f64 = 190.0;
    let left = 24.0;
    let width = WIDTH - left - RIGHT_ROOM;
    let x_of = |percent: f64| left + (percent.clamp(-last, last) + last) / (2.0 * last) * width;
    let title = format!("A/B: {} against {}", ab.b, ab.a);
    let mut description = format!(
        "Each metric's change of its median from {} to {}, in percent of {}'s:",
        ab.a, ab.b, ab.a
    );
    for (index, (_, group, change, percent)) in rows.iter().enumerate() {
        let _ = write!(
            description,
            "{} {} {}, {percent:+.1} %, its interval from {:+.1} to {:+.1} %, {}",
            if index == 0 { "" } else { ";" },
            group.tool,
            group.heading(),
            percent_of(change.interval.0, change.a),
            percent_of(change.interval.1, change.a),
            change.verdict.words()
        );
    }
    description.push('.');
    let mut chart = Chart::new(
        "ab",
        &title,
        &format!(
            "Each metric's change of its median, {}'s less {}'s, in percent of {}'s, a dot, and its 95 % interval, a line: a change whose interval holds 0 is within the noise",
            ab.b, ab.a, ab.a
        ),
        &description,
    );
    let dot = |class: &'static str| {
        move |x: f64, y: f64| {
            format!(
                r#"<circle class="{class} ring" cx="{x:.1}" cy="{y:.1}" r="5" stroke-width="2"/>"#
            )
        }
    };
    let (better, worse, plain) = (dot("better"), dot("worse"), dot("context"));
    let mut items: Vec<Item<'_>> = vec![
        (&better, "better".to_owned()),
        (&worse, "worse".to_owned()),
        (&plain, "within the noise".to_owned()),
    ];
    if limits.iter().any(|row| !row.is_empty()) {
        items.push((&budget_mark, "budget".to_owned()));
    }
    let legend_y = chart.top() + 2.0;
    legend(&mut chart, legend_y, &items);
    const ROW: f64 = 46.0;
    let top = chart.top() + 26.0;
    let bottom = top + ROW * rows.len() as f64 - 6.0;
    vertical_axis(&mut chart, &both, &x_of, (top, bottom), -1.0, " %");
    chart.push(&format!(
        r#"<line class="base" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{bottom:.1}" stroke-width="1"/>"#,
        x_of(0.0),
        top - 6.0,
        x_of(0.0)
    ));
    for (index, (at, group, change, percent)) in rows.iter().enumerate() {
        let row_top = top + index as f64 * ROW;
        let y = row_top + 28.0;
        let class = match change.verdict {
            Verdict::Better => "better",
            Verdict::Worse => "worse",
            Verdict::Higher | Verdict::Lower => "plain",
            Verdict::WithinNoise | Verdict::TooFew => "context",
        };
        let label = format!("{} · {} ({})", group.tool, group.heading(), group.unit);
        chart.push(&format!(
            r#"<text class="secondary halo" x="{left:.1}" y="{:.1}" font-size="12">{}</text>"#,
            row_top + 10.0,
            escape(&label)
        ));
        let (low, high) = (
            percent_of(change.interval.0, change.a),
            percent_of(change.interval.1, change.a),
        );
        chart.push(&format!(
            r#"<line class="{class}-line" x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke-width="2" stroke-linecap="round"/>"#,
            x_of(low),
            x_of(high)
        ));
        let mut hit = format!(
            "{label}: {} to {}, {percent:+.1} %, its interval {low:+.1} to {high:+.1} %, {}",
            change.a,
            change.b,
            change.verdict.words()
        );
        for (limit, budget) in &limits[index] {
            let x = x_of(*limit);
            let _ = write!(
                hit,
                "; budget {} {}",
                budget.describe_changes(&group.unit),
                held_words(held(report, *at, None, budget))
            );
            chart.push(&format!(
                r#"<line class="budget" x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{:.1}" stroke-width="2"/>"#,
                y - 8.0,
                y + 8.0
            ));
        }
        chart.push(&format!(
            r#"<circle class="{class} ring" cx="{:.1}" cy="{y:.1}" r="5" stroke-width="2"/>"#,
            x_of(*percent)
        ));
        let weight = if change.verdict == Verdict::Worse {
            r#" font-weight="600""#
        } else {
            ""
        };
        chart.push(&format!(
            r#"<text class="primary tick" x="{:.1}" y="{:.1}" font-size="12"{weight}>{percent:+.1} %<tspan class="secondary" font-weight="400"> · {}</tspan></text>"#,
            left + width + 16.0,
            y + 4.0,
            change.verdict.words()
        ));
        chart.push(&format!(
            r#"<rect class="hit" x="0" y="{row_top:.1}" width="{WIDTH}" height="{ROW}"><title>{}</title></rect>"#,
            escape(&hit)
        ));
    }
    let svg = chart.finish(bottom + 6.0 + AXIS);
    Some(Drawn {
        file: "ab.svg".to_owned(),
        title,
        kind: "ab",
        groups: rows.iter().map(|(at, ..)| *at).collect(),
        svg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summary(median: f64, p95: f64, count: usize) -> serde_json::Value {
        json!({
            "count": count, "min": median - 1.0, "median": median, "p95": p95,
            "p99": p95, "max": p95, "mean": median, "stddev": 1.0,
            "median_interval": [median - 1.0, median + 1.0]
        })
    }

    fn group(tool: &str, metric: &str, conditions: &serde_json::Value) -> serde_json::Value {
        json!({
            "tool": tool, "metric": metric, "unit": "ms", "title": format!("{metric} <&>"),
            "feature": null, "conditions": conditions, "telling": [],
            "rows": [
                {"terminal": "old", "summary": summary(10.5, 12.5, 4)},
                {"terminal": "new", "summary": summary(9.5, 11.5, 4)}
            ],
            "change": {"a": 10.5, "b": 9.5, "change": -1.0, "percent": -9.52,
                       "interval": [-2.0, -0.5], "verdict": "better"}
        })
    }

    fn record(tool: &str, metric: &str, terminal: &str, samples: &[f64]) -> Record {
        Record {
            tool: tool.to_owned(),
            terminal: terminal.to_owned(),
            version: None,
            metric: metric.to_owned(),
            unit: "ms".to_owned(),
            samples: samples.to_vec(),
            conditions: serde_json::Map::new(),
            cycle: None,
        }
    }

    /// Whether `svg` is well-formed XML.
    fn well_formed(svg: &str) -> bool {
        let mut reader = quick_xml::Reader::from_str(svg);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
    }

    #[test]
    fn each_group_of_its_kind_valid_and_named_once() {
        let report: Report = serde_json::from_value(json!({
            "session": "s",
            "terminals": [{"name": "old"}, {"name": "new"}],
            "ab": {"a": "old", "b": "new"},
            "groups": [
                group("startup", "ready_ms", &json!({})),
                group("latency", "latency_ms", &json!({})),
                group("frames", "frame_gap_ms", &json!({"refresh_hz": 60.0})),
                group("gpu", "terminal_gpu_ms_per_s", &json!({})),
                group("gpu", "window_server_gpu_ms_per_s", &json!({})),
                group("gpu", "window_server_gpu_ms_per_s", &json!({"workload": "rest"})),
                group("startup", "ready_ms", &json!({})),
            ],
            "budgets": [{"metric": "ready_ms", "at-most": 11.0},
                        {"metric": "ready_ms", "change-at-most": "5%"}],
            "judgements": [
                {"budget": 1, "group": 0, "terminal": "old", "held": true},
                {"budget": 1, "group": 0, "terminal": "new", "held": true},
                {"budget": 2, "group": 0, "terminal": null, "held": true}
            ]
        }))
        .unwrap();
        let records = [
            record("latency", "latency_ms", "old", &[10.0, 12.5, 11.0, 30.0]),
            record("latency", "latency_ms", "new", &[9.0, 9.5, 10.0, 11.0]),
        ];
        let mut gaps = record("frames", "frame_gap_ms", "old", &[16.6, 16.8, 33.3, 16.7]);
        gaps.conditions.insert("refresh_hz".into(), 60.0.into());
        let records = [records[0].clone(), records[1].clone(), gaps];
        let drawn = all(&report, &records);
        let kinds: Vec<(&str, &str, &[usize])> = drawn
            .iter()
            .map(|chart| (chart.file.as_str(), chart.kind, chart.groups.as_slice()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("ab.svg", "ab", &[0, 1, 2, 3, 4, 5, 6][..]),
                ("startup-ready-ms.svg", "bars", &[0][..]),
                ("latency-latency-ms.svg", "distribution", &[1][..]),
                ("frames-frame-gap-ms-time.svg", "intervals", &[2][..]),
                ("gpu-terminal-gpu-ms-per-s-stacked.svg", "gpu", &[3, 4][..]),
                ("gpu-window-server-gpu-ms-per-s.svg", "bars", &[5][..]),
                ("startup-ready-ms-2.svg", "bars", &[6][..]),
            ]
        );
        for chart in &drawn {
            assert!(well_formed(&chart.svg), "{}:\n{}", chart.file, chart.svg);
            let stem = chart.file.trim_end_matches(".svg");
            assert!(
                chart
                    .svg
                    .contains(&format!(r#"aria-labelledby="{stem}-title {stem}-desc""#)),
                "{}",
                chart.file
            );
            assert!(chart.svg.contains(&format!(r#"<title id="{stem}-title">"#)));
            assert!(chart.svg.contains("prefers-color-scheme: dark"));
        }
        // The title's text is escaped; the budgets are drawn and told.
        let bars = &drawn[1].svg;
        assert!(bars.contains("ready_ms &lt;&amp;&gt;"));
        assert!(bars.contains("budget ≤ 11 ms held"));
        assert!(drawn[0].svg.contains("budget ≤ +5 % held"));
        assert!(drawn[3].svg.contains("a refresh, 16.7 ms"));
        // Each terminal of its color, in the session's order.
        assert!(bars.contains(r#"<path class="t0""#) && bars.contains(r#"<path class="t1""#));
    }

    #[test]
    fn no_ab_chart_of_no_ab_and_no_bars_of_negative_values() {
        let mut negative = group("startup", "ready_ms", &json!({}));
        negative["rows"][0]["summary"]["min"] = json!(-1.0);
        let report: Report = serde_json::from_value(json!({
            "session": "s",
            "terminals": [{"name": "old"}],
            "groups": [negative],
        }))
        .unwrap();
        assert!(all(&report, &[]).is_empty());
    }
}
