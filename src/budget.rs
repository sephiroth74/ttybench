//! What a session's file asks of its measures besides running them: an
//! A/B of two of its terminals, as two copies of one, and budgets, each a
//! limit of a metric's statistic, or of its change from A to B, which the
//! report tells held or not.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::stats::Stat;

/// Two terminals of a session compared: A, the copy before, as a release,
/// and B, the copy after, as its next build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ab {
    /// The copy before.
    pub a: String,
    /// The copy after.
    pub b: String,
}

/// A limit of a change from A to B: in the metric's unit, or a share of
/// A's value, written as a text such as `"5%"`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ChangeValue", into = "ChangeValue")]
pub enum Change {
    /// In the metric's unit.
    Units(f64),
    /// In percent of A's value.
    Percent(f64),
}

/// A change's limit as a session's file writes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum ChangeValue {
    Number(f64),
    Text(String),
}

impl TryFrom<ChangeValue> for Change {
    type Error = String;

    fn try_from(value: ChangeValue) -> Result<Self, String> {
        let change = match value {
            ChangeValue::Number(number) => Self::Units(number),
            ChangeValue::Text(text) => {
                let number = text
                    .trim()
                    .strip_suffix('%')
                    .map(str::trim_end)
                    .and_then(|number| number.parse::<f64>().ok())
                    .ok_or_else(|| {
                        format!("a change is a number, or a share as \"5%\", not \"{text}\"")
                    })?;
                Self::Percent(number)
            }
        };
        if change.value().is_finite() {
            Ok(change)
        } else {
            Err("a change is a finite number".to_owned())
        }
    }
}

impl From<Change> for ChangeValue {
    fn from(change: Change) -> Self {
        match change {
            Change::Units(number) => Self::Number(number),
            Change::Percent(number) => Self::Text(format!("{number}%")),
        }
    }
}

impl Change {
    /// Its number, of its unit or in percent.
    pub fn value(self) -> f64 {
        match self {
            Self::Units(number) | Self::Percent(number) => number,
        }
    }

    /// The change from `a` to `b` as this limit counts it: none of a share
    /// of an `a` of 0.
    pub fn of(self, a: f64, b: f64) -> Option<f64> {
        match self {
            Self::Units(_) => Some(b - a),
            Self::Percent(_) => (a != 0.0).then(|| (b - a) / a.abs() * 100.0),
        }
    }

    /// The limit as a text, of `unit`.
    pub fn describe(self, unit: &str) -> String {
        match self {
            Self::Units(number) => of_unit(&format!("{number:+}"), unit),
            Self::Percent(number) => format!("{number:+} %"),
        }
    }
}

/// A budget: a limit of a metric's statistic, of each terminal that it
/// names, or of its change from the A/B's A to its B.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Budget {
    /// The metric, as `ready_ms`.
    pub metric: String,
    /// The tool, as `startup`; any of none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The terminal that a limit of the statistic holds for; every one of
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// The conditions of the records that it judges, each of the value
    /// that a record's condition of its name has, as `{ text = "plain" }`;
    /// every record of none.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub conditions: Map<String, Value>,
    /// The statistic, the median by default.
    #[serde(default)]
    pub stat: Stat,
    /// The statistic's most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<f64>,
    /// The statistic's least.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_least: Option<f64>,
    /// The most of the statistic's change from A to B.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_at_most: Option<Change>,
    /// The least of the statistic's change from A to B.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_at_least: Option<Change>,
}

impl Budget {
    /// Why the budget asks nothing that a report can tell, if it does not.
    pub fn check(&self) -> Result<(), String> {
        let which = format!("the budget of {}", self.metric);
        if self.metric.is_empty() {
            return Err("a budget names its metric".to_owned());
        }
        if !self.limits() && !self.changes() {
            return Err(format!(
                "{which} has no limit: at-most, at-least, change-at-most or change-at-least"
            ));
        }
        for (name, value) in [("at-most", self.at_most), ("at-least", self.at_least)] {
            if value.is_some_and(|value| !value.is_finite()) {
                return Err(format!("{which}: {name} is a finite number"));
            }
        }
        if let (Some(most), Some(least)) = (self.at_most, self.at_least)
            && least > most
        {
            return Err(format!("{which}: at-least {least} is above at-most {most}"));
        }
        if let (Some(most), Some(least)) = (self.change_at_most, self.change_at_least)
            && std::mem::discriminant(&most) == std::mem::discriminant(&least)
            && least.value() > most.value()
        {
            return Err(format!("{which}: change-at-least is above change-at-most"));
        }
        if self.changes() && self.terminal.is_some() {
            return Err(format!(
                "{which}: a change's limit is of the A/B's two terminals, of no terminal of its own"
            ));
        }
        Ok(())
    }

    /// Whether it limits the statistic of a terminal.
    pub fn limits(&self) -> bool {
        self.at_most.is_some() || self.at_least.is_some()
    }

    /// Whether it limits the statistic's change from A to B.
    pub fn changes(&self) -> bool {
        self.change_at_most.is_some() || self.change_at_least.is_some()
    }

    /// Whether it judges the records of `tool`, `metric` and `conditions`.
    pub fn judges(&self, tool: &str, metric: &str, conditions: &Map<String, Value>) -> bool {
        metric == self.metric
            && self.tool.as_deref().is_none_or(|own| own == tool)
            && self
                .conditions
                .iter()
                .all(|(key, value)| conditions.get(key).is_some_and(|other| same(value, other)))
    }

    /// Whether it judges `terminal`'s statistic.
    pub fn judges_terminal(&self, terminal: &str) -> bool {
        self.terminal.as_deref().is_none_or(|own| own == terminal)
    }

    /// Whether `value` holds within its limits of the statistic.
    pub fn holds(&self, value: f64) -> bool {
        self.at_most.is_none_or(|most| value <= most)
            && self.at_least.is_none_or(|least| value >= least)
    }

    /// Whether the change from `a` to `b` holds within its limits of the
    /// change; none of a share of an `a` of 0.
    pub fn change_holds(&self, a: f64, b: f64) -> Option<bool> {
        let mut holds = true;
        if let Some(most) = self.change_at_most {
            holds &= most.of(a, b)? <= most.value();
        }
        if let Some(least) = self.change_at_least {
            holds &= least.of(a, b)? >= least.value();
        }
        Some(holds)
    }

    /// Its limits of the statistic, as a text, of `unit`.
    pub fn describe_limits(&self, unit: &str) -> String {
        let mut parts = Vec::new();
        if let Some(least) = self.at_least {
            parts.push(format!("≥ {}", of_unit(&least.to_string(), unit)));
        }
        if let Some(most) = self.at_most {
            parts.push(format!("≤ {}", of_unit(&most.to_string(), unit)));
        }
        parts.join(" and ")
    }

    /// Its limits of the change, as a text, of `unit`.
    pub fn describe_changes(&self, unit: &str) -> String {
        let mut parts = Vec::new();
        if let Some(least) = self.change_at_least {
            parts.push(format!("≥ {}", least.describe(unit)));
        }
        if let Some(most) = self.change_at_most {
            parts.push(format!("≤ {}", most.describe(unit)));
        }
        parts.join(" and ")
    }
}

/// `number` of `unit`, if one.
fn of_unit(number: &str, unit: &str) -> String {
    if unit.is_empty() {
        number.to_owned()
    } else {
        format!("{number} {unit}")
    }
}

/// Whether two conditions' values are the same, a number whatever its
/// type, as `10` and `10.0`.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

/// What a report takes of a session's file: its terminals, its A/B and
/// its budgets, the rest the coordinator's.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Judged {
    /// Its terminals, in their order.
    #[serde(default)]
    pub terminals: Vec<String>,
    /// Its A/B, if one.
    #[serde(default)]
    pub ab: Option<Ab>,
    /// Its budgets.
    #[serde(default, rename = "budget")]
    pub budgets: Vec<Budget>,
}

/// Why the A/B `ab` and the budgets `budgets` do not fit a session of the
/// terminals `terminals`, if they do not.
pub fn check(ab: Option<&Ab>, budgets: &[Budget], terminals: &[String]) -> Result<(), String> {
    let known = |name: &str, what: &str| {
        if terminals.iter().any(|known| known == name) {
            Ok(())
        } else {
            Err(format!(
                "{what} names `{name}`, which is not among the session's terminals"
            ))
        }
    };
    if let Some(ab) = ab {
        known(&ab.a, "the A/B")?;
        known(&ab.b, "the A/B")?;
        if ab.a == ab.b {
            return Err(format!(
                "the A/B compares `{}` with itself: a and b are two terminals",
                ab.a
            ));
        }
    }
    for budget in budgets {
        budget.check()?;
        if let Some(terminal) = &budget.terminal {
            known(terminal, &format!("the budget of {}", budget.metric))?;
        }
        if budget.changes() && ab.is_none() {
            return Err(format!(
                "the budget of {} limits a change, of no A/B: ab = {{ a = \"<before>\", b = \"<after>\" }}",
                budget.metric
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judged(text: &str) -> Result<Judged, String> {
        toml::from_str(text).map_err(|error| error.message().to_owned())
    }

    fn terminals() -> Vec<String> {
        vec!["old".to_owned(), "new".to_owned()]
    }

    #[test]
    fn a_sessions_budgets_and_its_ab() {
        let judged = judged(
            r#"
terminals = ["old", "new"]
ab = { a = "old", b = "new" }

[[budget]]
tool = "startup"
metric = "ready_ms"
at-most = 300

[[budget]]
metric = "written_mib_s"
conditions = { text = "plain", mib = 256 }
stat = "p95"
change-at-least = "-5%"
change-at-most = 20
"#,
        )
        .unwrap();
        let ab = judged.ab.clone().unwrap();
        assert_eq!((ab.a.as_str(), ab.b.as_str()), ("old", "new"));
        check(Some(&ab), &judged.budgets, &terminals()).unwrap();
        let [startup, throughput] = judged.budgets.as_slice() else {
            panic!("{:?}", judged.budgets);
        };
        assert_eq!(startup.stat, Stat::Median);
        assert!(startup.holds(300.0) && !startup.holds(300.5));
        assert_eq!(startup.describe_limits("ms"), "≤ 300 ms");
        assert_eq!(throughput.stat, Stat::P95);
        assert_eq!(throughput.change_at_least, Some(Change::Percent(-5.0)));
        assert_eq!(throughput.change_at_most, Some(Change::Units(20.0)));
        assert_eq!(throughput.change_holds(100.0, 96.0), Some(true));
        assert_eq!(throughput.change_holds(100.0, 94.0), Some(false));
        assert_eq!(throughput.change_holds(100.0, 121.0), Some(false));
        assert_eq!(throughput.change_holds(0.0, 1.0), None);
        assert_eq!(
            throughput.describe_changes("MiB/s"),
            "≥ -5 % and ≤ +20 MiB/s"
        );
        // Conditions of a number match whatever its type, and the others
        // of their value alone.
        let mut conditions = Map::new();
        conditions.insert("text".into(), "plain".into());
        conditions.insert("mib".into(), 256.0.into());
        conditions.insert("rounds".into(), 3.into());
        assert!(throughput.judges("throughput", "written_mib_s", &conditions));
        conditions.insert("text".into(), "colors".into());
        assert!(!throughput.judges("throughput", "written_mib_s", &conditions));
        assert!(startup.judges("startup", "ready_ms", &Map::new()));
        assert!(!startup.judges("other", "ready_ms", &Map::new()));
    }

    #[test]
    fn what_a_session_refuses() {
        for (text, refusal) in [
            ("[[budget]]\nmetric = \"x\"\n", "has no limit"),
            (
                "[[budget]]\nmetric = \"x\"\nat-most = 1\nat-least = 2\n",
                "above at-most",
            ),
            (
                "[[budget]]\nmetric = \"x\"\nchange-at-most = 1\n",
                "of no A/B",
            ),
            (
                "ab = { a = \"old\", b = \"new\" }\n[[budget]]\nmetric = \"x\"\nterminal = \"old\"\nchange-at-most = 1\n",
                "of no terminal of its own",
            ),
            (
                "[[budget]]\nmetric = \"x\"\nterminal = \"other\"\nat-most = 1\n",
                "not among the session's terminals",
            ),
            ("ab = { a = \"old\", b = \"old\" }\n", "with itself"),
            ("ab = { a = \"old\", b = \"other\" }\n", "`other`"),
        ] {
            let judged = judged(text).unwrap();
            let error = check(judged.ab.as_ref(), &judged.budgets, &terminals()).unwrap_err();
            assert!(error.contains(refusal), "{text}: {error}");
        }
        for (text, refusal) in [
            (
                "[[budget]]\nmetric = \"x\"\nchange-at-most = \"five\"\n",
                "a share as",
            ),
            (
                "[[budget]]\nmetric = \"x\"\nat-most = 1\nmost = 2\n",
                "most",
            ),
            ("ab = { a = \"old\", b = \"new\", c = \"x\" }\n", "c"),
        ] {
            let error = judged(text).unwrap_err();
            assert!(error.contains(refusal), "{text}: {error}");
        }
    }
}
