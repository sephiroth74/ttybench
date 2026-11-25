//! What the charts read of a session: its report's JSON, `report.json`,
//! as `ttybench report` writes it, of the fields that they draw, and each
//! group's samples of its records, which the report does not keep.

use serde::Deserialize;
use serde_json::{Map, Value};
use ttybench::budget::{Ab, Budget};
use ttybench::results::Record;
use ttybench::stats::Summary;

/// A session's report.
#[derive(Clone, Debug, Deserialize)]
pub struct Report {
    /// The session's name.
    pub session: String,
    /// Its terminals, in their order.
    pub terminals: Vec<Terminal>,
    /// Its A/B, if one.
    #[serde(default)]
    pub ab: Option<Ab>,
    /// Each metric of each tool and conditions.
    pub groups: Vec<Group>,
    /// The session's budgets.
    #[serde(default)]
    pub budgets: Vec<Budget>,
    /// The budgets judged.
    #[serde(default)]
    pub judgements: Vec<Judgement>,
}

impl Report {
    /// The place of the terminal `name` in the session's, which its color
    /// follows in every chart.
    pub fn place(&self, name: &str) -> usize {
        self.terminals
            .iter()
            .position(|terminal| terminal.name == name)
            .unwrap_or(usize::MAX)
    }
}

/// A terminal of the session.
#[derive(Clone, Debug, Deserialize)]
pub struct Terminal {
    /// Its name.
    pub name: String,
    /// Its version, if its records tell one.
    #[serde(default)]
    pub version: Option<String>,
}

/// A metric of a tool, of records of the same conditions.
#[derive(Clone, Debug, Deserialize)]
pub struct Group {
    /// The tool.
    pub tool: String,
    /// The metric.
    pub metric: String,
    /// Its unit.
    pub unit: String,
    /// Its title.
    pub title: String,
    /// The feature that its records measured, if one.
    #[serde(default)]
    pub feature: Option<String>,
    /// Its records' conditions.
    #[serde(default)]
    pub conditions: Map<String, Value>,
    /// The conditions that tell it from the tool's other groups of its
    /// metric.
    #[serde(default)]
    pub telling: Vec<String>,
    /// Each terminal's summary.
    pub rows: Vec<Row>,
    /// The A/B's change, if one.
    #[serde(default)]
    pub change: Option<Change>,
}

impl Group {
    /// What tells it apart after its title: its feature and its telling
    /// conditions, as `of smooth-scroll, text plain`; empty of none.
    pub fn qualifier(&self) -> String {
        let mut parts = Vec::new();
        if let Some(feature) = &self.feature {
            parts.push(format!("of {feature}"));
        }
        for name in &self.telling {
            if let Some(value) = self.conditions.get(name) {
                parts.push(format!("{name} {}", value_text(value)));
            }
        }
        parts.join(", ")
    }

    /// Its title and its qualifier.
    pub fn heading(&self) -> String {
        match self.qualifier() {
            qualifier if qualifier.is_empty() => self.title.clone(),
            qualifier => format!("{}, {qualifier}", self.title),
        }
    }

    /// The display's refresh rate of its conditions, if they tell one.
    pub fn refresh_hz(&self) -> Option<f64> {
        self.conditions.get("refresh_hz").and_then(Value::as_f64)
    }

    /// Its samples of `terminal`, of `records`, in their order.
    pub fn samples(&self, terminal: &str, records: &[Record]) -> Vec<f64> {
        records
            .iter()
            .filter(|record| {
                record.terminal == terminal
                    && record.tool == self.tool
                    && record.metric == self.metric
                    && record.unit == self.unit
                    && record.conditions == self.conditions
            })
            .flat_map(|record| record.samples.iter().copied())
            .filter(|sample| sample.is_finite())
            .collect()
    }
}

/// A condition's value as a text: a text of no quotes, a number of three
/// decimals at most.
pub fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) if number.is_f64() => {
            let text = format!("{:.3}", number.as_f64().unwrap_or_default());
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
        other => other.to_string(),
    }
}

/// A terminal's summary of a group.
#[derive(Clone, Debug, Deserialize)]
pub struct Row {
    /// The terminal.
    pub terminal: String,
    /// Its samples' summary.
    pub summary: Summary,
}

/// The A/B's change of a group's median.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Change {
    /// A's median.
    pub a: f64,
    /// B's median.
    pub b: f64,
    /// In percent of A's, of an A not 0.
    #[serde(default)]
    pub percent: Option<f64>,
    /// Its 95 % bootstrap interval.
    pub interval: (f64, f64),
    /// What it tells, as `within-noise`.
    pub verdict: Verdict,
}

/// What a change tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Better beyond the noise.
    Better,
    /// Worse beyond the noise.
    Worse,
    /// Higher beyond the noise.
    Higher,
    /// Lower beyond the noise.
    Lower,
    /// Within the noise.
    WithinNoise,
    /// Too few samples to tell.
    TooFew,
}

impl Verdict {
    /// Its words.
    pub fn words(self) -> &'static str {
        match self {
            Self::Better => "better",
            Self::Worse => "worse",
            Self::Higher => "higher",
            Self::Lower => "lower",
            Self::WithinNoise => "within the noise",
            Self::TooFew => "too few runs",
        }
    }
}

/// A budget judged.
#[derive(Clone, Debug, Deserialize)]
pub struct Judgement {
    /// The budget's place in the session's file, from 1.
    pub budget: usize,
    /// The group that it judged, if one.
    #[serde(default)]
    pub group: Option<usize>,
    /// The terminal, or none of the A/B's change.
    #[serde(default)]
    pub terminal: Option<String>,
    /// Whether it held, if anything was there to judge.
    #[serde(default)]
    pub held: Option<bool>,
}
