//! A session's style of its report and its charts, `style.toml`: which
//! charts it draws and of which metrics, their width, font and colors, of
//! each appearance and of each terminal; the report's tables' columns,
//! their titles and the metrics' units; and each metric's statistic of the
//! A/B. Every key is optional, the built-in style its defaults; a
//! session's file names one, which the coordinator copies into the
//! session's folder, where the report and the charts find it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::config::{self, ConfigError};
use crate::stats::Stat;

/// The style's copy in a session's folder.
pub const FILE: &str = "style.toml";

/// The kinds of charts that a session draws, by their names.
pub const KINDS: [&str; 5] = ["bars", "distribution", "intervals", "gpu", "ab"];

/// The columns of a metric's table, by their names, in the built-in
/// style's order: the median, its 95 % interval, the 95th percentile, the
/// mean and its deviation, the least and the greatest, the samples and the
/// runs, and each cycle's median.
pub const COLUMNS: [&str; 7] = [
    "median", "interval", "p95", "mean", "range", "samples", "cycles",
];

/// The metrics compared of their means in an A/B unless a style says
/// otherwise: events so rare that both medians are 0, whose mean is the
/// rate.
const MEAN_METRICS: [&str; 2] = [
    "idle.interrupt_wakeups_per_s",
    "idle.package_idle_wakeups_per_s",
];

/// The least and the greatest width of a chart, in points.
const WIDTHS: (f64, f64) = (320.0, 2000.0);

/// A style, as its file writes it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Style {
    /// The charts'.
    #[serde(default)]
    pub charts: Charts,
    /// The report's.
    #[serde(default)]
    pub report: Report,
    /// Each metric's statistic of the A/B, by `tool.metric`, as
    /// `idle.cpu_percent = "mean"`: the median of the others, but the
    /// wakeups', of their means.
    #[serde(default)]
    pub ab: BTreeMap<String, Stat>,
}

/// The charts' style.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Charts {
    /// The kinds that a session draws, of [`KINDS`]; all of them of none.
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    /// The metrics drawn, by `tool.metric`; all of them of none.
    #[serde(default)]
    pub metrics: Option<Vec<String>>,
    /// A chart's width, in points.
    #[serde(default)]
    pub width: Option<f64>,
    /// The font's family, as CSS writes it.
    #[serde(default)]
    pub font: Option<String>,
    /// The colors of the light appearance in place of the built-in ones,
    /// by their names, as `surface = "#ffffff"`.
    #[serde(default)]
    pub light: BTreeMap<String, String>,
    /// The dark appearance's.
    #[serde(default)]
    pub dark: BTreeMap<String, String>,
    /// Each terminal's own color, by its name, in place of the series'
    /// color of its place.
    #[serde(default)]
    pub terminals: BTreeMap<String, TerminalColor>,
}

/// A terminal's color in each appearance.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalColor {
    /// In the light appearance.
    pub light: String,
    /// In the dark appearance; the light one's of none.
    #[serde(default)]
    pub dark: Option<String>,
}

/// The report's style.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Report {
    /// A metric's table's columns, of [`COLUMNS`], in their order; all of
    /// them of none.
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    /// A column's title in place of the built-in one, by its name.
    #[serde(default)]
    pub titles: BTreeMap<String, String>,
    /// A metric's unit in place of its tool's, by `tool.metric`.
    #[serde(default)]
    pub units: BTreeMap<String, String>,
}

impl Style {
    /// The style of the file at `path`, checked.
    pub fn read(path: &Path) -> Result<Self, String> {
        let style: Self = config::read(path).map_err(|error: ConfigError| error.to_string())?;
        style
            .check()
            .map_err(|why| format!("{}: {why}", path.display()))?;
        Ok(style)
    }

    /// The style of a session's `folder`: the file `given`, else the
    /// folder's copy, else the built-in one.
    pub fn of_session(folder: &Path, given: Option<&Path>) -> Result<Self, String> {
        match given {
            Some(path) => Self::read(path),
            None if folder.join(FILE).exists() => Self::read(&folder.join(FILE)),
            None => Ok(Self::default()),
        }
    }

    /// Why the style is wrong, if it is: a kind or a column of no name of
    /// theirs or twice, a metric not `tool.metric`, a width out of its
    /// range, a color not `#rrggbb`.
    pub fn check(&self) -> Result<(), String> {
        if let Some(kinds) = &self.charts.kinds {
            names(kinds, &KINDS, "charts.kinds")?;
        }
        if let Some(columns) = &self.report.columns {
            names(columns, &COLUMNS, "report.columns")?;
        }
        for name in self.report.titles.keys() {
            if !COLUMNS.contains(&name.as_str()) {
                return Err(format!(
                    "report.titles: `{name}` is no column, of {}",
                    COLUMNS.join(", ")
                ));
            }
        }
        let metrics = self
            .charts
            .metrics
            .iter()
            .flatten()
            .map(|metric| ("charts.metrics", metric))
            .chain(
                self.report
                    .units
                    .keys()
                    .map(|metric| ("report.units", metric)),
            )
            .chain(self.ab.keys().map(|metric| ("ab", metric)));
        for (key, metric) in metrics {
            if !is_metric(metric) {
                return Err(format!("{key}: `{metric}` is not `tool.metric`"));
            }
        }
        if let Some(width) = self.charts.width
            && !(WIDTHS.0..=WIDTHS.1).contains(&width)
        {
            return Err(format!(
                "charts.width: {width} is not from {} to {} points",
                WIDTHS.0, WIDTHS.1
            ));
        }
        let colors = self
            .charts
            .light
            .iter()
            .map(|(name, color)| (format!("charts.light.{name}"), color))
            .chain(
                self.charts
                    .dark
                    .iter()
                    .map(|(name, color)| (format!("charts.dark.{name}"), color)),
            )
            .chain(self.charts.terminals.iter().flat_map(|(name, colors)| {
                std::iter::once((format!("charts.terminals.{name}.light"), &colors.light)).chain(
                    colors
                        .dark
                        .iter()
                        .map(move |dark| (format!("charts.terminals.{name}.dark"), dark)),
                )
            }));
        for (key, color) in colors {
            if !is_color(color) {
                return Err(format!("{key}: `{color}` is not a color of `#rrggbb`"));
            }
        }
        if let Some(font) = &self.charts.font
            && (font.is_empty() || font.contains(['<', '>', '{', '}', ';']))
        {
            return Err(format!("charts.font: `{font}` is no font's family"));
        }
        Ok(())
    }

    /// The A/B's statistic of the metric `metric` of `tool`.
    pub fn ab_stat(&self, tool: &str, metric: &str) -> Stat {
        let key = format!("{tool}.{metric}");
        self.ab.get(&key).copied().unwrap_or_else(|| {
            if MEAN_METRICS.contains(&key.as_str()) {
                Stat::Mean
            } else {
                Stat::Median
            }
        })
    }

    /// Whether the charts of the kind `kind` are drawn.
    pub fn draws_kind(&self, kind: &str) -> bool {
        self.charts
            .kinds
            .as_ref()
            .is_none_or(|kinds| kinds.iter().any(|drawn| drawn == kind))
    }

    /// Whether the metric `metric` of `tool` is drawn.
    pub fn draws_metric(&self, tool: &str, metric: &str) -> bool {
        self.charts.metrics.as_ref().is_none_or(|metrics| {
            metrics
                .iter()
                .any(|drawn| drawn.split_once('.') == Some((tool, metric)))
        })
    }

    /// A metric's table's columns, in their order.
    pub fn columns(&self) -> Vec<&str> {
        self.report.columns.as_ref().map_or_else(
            || COLUMNS.to_vec(),
            |columns| columns.iter().map(String::as_str).collect(),
        )
    }

    /// The title of the column `column`, else `built_in`.
    pub fn title<'a>(&'a self, column: &str, built_in: &'a str) -> &'a str {
        self.report
            .titles
            .get(column)
            .map_or(built_in, String::as_str)
    }

    /// The unit of the metric `metric` of `tool`, else `built_in`.
    pub fn unit<'a>(&'a self, tool: &str, metric: &str, built_in: &'a str) -> &'a str {
        self.report
            .units
            .get(&format!("{tool}.{metric}"))
            .map_or(built_in, String::as_str)
    }
}

/// Why `given` is not a list of `known` names, each once, if it is not.
fn names(given: &[String], known: &[&str], key: &str) -> Result<(), String> {
    for (at, name) in given.iter().enumerate() {
        if !known.contains(&name.as_str()) {
            return Err(format!(
                "{key}: `{name}` is not one of {}",
                known.join(", ")
            ));
        }
        if given[..at].contains(name) {
            return Err(format!("{key}: `{name}` twice"));
        }
    }
    Ok(())
}

/// Whether `metric` is `tool.metric`, of lower case letters, digits, `-`
/// and `_`.
fn is_metric(metric: &str) -> bool {
    let part = |text: &str| {
        !text.is_empty()
            && text.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte)
            })
    };
    metric
        .split_once('.')
        .is_some_and(|(tool, name)| part(tool) && part(name))
}

/// Whether `color` is `#rrggbb`.
fn is_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(text: &str) -> Result<Style, String> {
        let style: Style =
            config::parse(Path::new("style.toml"), text).map_err(|error| error.to_string())?;
        style.check()?;
        Ok(style)
    }

    #[test]
    fn the_built_in_style_draws_everything_of_every_column() {
        let built_in = Style::default();
        assert!(KINDS.iter().all(|kind| built_in.draws_kind(kind)));
        assert!(built_in.draws_metric("startup", "window_ms"));
        assert_eq!(built_in.columns(), COLUMNS);
        assert_eq!(built_in.title("median", "Median"), "Median");
        assert_eq!(built_in.unit("memory", "footprint_mib", "MiB"), "MiB");
        assert_eq!(built_in.ab_stat("startup", "window_ms"), Stat::Median);
        // The wakeups of their means, which both medians of 0 tell nothing of.
        assert_eq!(
            built_in.ab_stat("idle", "interrupt_wakeups_per_s"),
            Stat::Mean
        );
    }

    #[test]
    fn a_style_s_keys_choose_the_charts_the_columns_the_units_and_the_statistics() {
        let style = style(
            r##"
            [charts]
            kinds = ["bars", "ab"]
            metrics = ["throughput.quiet_mib_s"]
            width = 900
            font = "Iosevka, monospace"
            light = { surface = "#ffffff" }
            dark = { surface = "#000000" }
            terminals.ghostty = { light = "#7c3aed", dark = "#a78bfa" }

            [report]
            columns = ["median", "samples"]
            titles = { median = "Middle" }
            units = { "memory.footprint_mib" = "mebibytes" }

            [ab]
            "idle.cpu_percent" = "mean"
            "idle.interrupt_wakeups_per_s" = "median"
            "##,
        )
        .unwrap();
        assert!(style.draws_kind("ab") && !style.draws_kind("gpu"));
        assert!(style.draws_metric("throughput", "quiet_mib_s"));
        assert!(!style.draws_metric("startup", "window_ms"));
        assert_eq!(style.columns(), ["median", "samples"]);
        assert_eq!(style.title("median", "Median"), "Middle");
        assert_eq!(style.unit("memory", "footprint_mib", "MiB"), "mebibytes");
        assert_eq!(style.ab_stat("idle", "cpu_percent"), Stat::Mean);
        assert_eq!(
            style.ab_stat("idle", "interrupt_wakeups_per_s"),
            Stat::Median
        );
        assert_eq!(
            style.charts.terminals["ghostty"].dark.as_deref(),
            Some("#a78bfa")
        );
    }

    #[test]
    fn the_example_style_holds_every_key_and_reads() {
        let example = style(include_str!("../../../examples/style.toml")).unwrap();
        assert!(example.charts.kinds.is_some() && example.charts.metrics.is_some());
        assert!(example.charts.width.is_some() && example.charts.font.is_some());
        assert!(!example.charts.light.is_empty() && !example.charts.dark.is_empty());
        assert!(!example.charts.terminals.is_empty());
        assert!(example.report.columns.is_some());
        assert!(!example.report.titles.is_empty() && !example.report.units.is_empty());
        assert!(!example.ab.is_empty());
    }

    #[test]
    fn a_wrong_style_tells_its_key() {
        for (text, key) in [
            ("[charts]\nkinds = [\"pie\"]", "charts.kinds"),
            ("[charts]\nkinds = [\"ab\", \"ab\"]", "charts.kinds"),
            ("[report]\ncolumns = [\"mode\"]", "report.columns"),
            ("[report]\ntitles = { mode = \"x\" }", "report.titles"),
            ("[charts]\nmetrics = [\"window_ms\"]", "charts.metrics"),
            ("[ab]\n\"Startup.window\" = \"mean\"", "ab"),
            ("[charts]\nwidth = 10", "charts.width"),
            (
                "[charts]\nlight = { surface = \"white\" }",
                "charts.light.surface",
            ),
            (
                "[charts]\nterminals.kitty = { light = \"#12345\" }",
                "charts.terminals.kitty.light",
            ),
            ("[charts]\nfont = \"x; color: red\"", "charts.font"),
        ] {
            let error = style(text).unwrap_err();
            assert!(error.contains(key), "{text}: {error}");
        }
        // An unknown key, or an unknown statistic, is the file's own error.
        assert!(style("[charts]\nsize = 3").is_err());
        assert!(style("[ab]\n\"idle.cpu_percent\" = \"mode\"").is_err());
    }
}
