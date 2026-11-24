//! A session's report as data: each metric's terminals summed up, of
//! records of the same conditions alone, the A/B's change of each metric,
//! and the budgets judged; which the Markdown and the JSON write.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};
use ttybench_core::budget::{Ab, Budget};
use ttybench_core::results::{self, Entry, Machine, Record, Status};
use ttybench_core::stats::{self, Stat, Summary};

use crate::metrics::{self, Better};

/// What a report is made of.
pub struct Input<'a> {
    /// The session's name, as its folder's.
    pub session: String,
    /// Its machine, if it tells it.
    pub machine: Option<Machine>,
    /// Its records.
    pub records: &'a [Record],
    /// The coordinator's log of its runs; none of a tool run alone.
    pub log: &'a [Entry],
    /// Its terminals in their order, of its file; else in the records'.
    pub terminals: &'a [String],
    /// Its A/B, if one.
    pub ab: Option<Ab>,
    /// Its budgets.
    pub budgets: &'a [Budget],
}

/// A session's report.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// The session's name.
    pub session: String,
    /// Its machine, if it tells it.
    pub machine: Option<Machine>,
    /// When its first run started and its last ended, in seconds since
    /// 1970, of the coordinator's log.
    pub span: Option<(u64, u64)>,
    /// How its runs ended, of the coordinator's log.
    pub runs: Option<Runs>,
    /// Its terminals, each of its version.
    pub terminals: Vec<Terminal>,
    /// Its A/B, if one.
    pub ab: Option<Ab>,
    /// Each metric of each tool and conditions, of its terminals.
    pub groups: Vec<Group>,
    /// The budgets judged.
    pub judgements: Vec<Judgement>,
    /// The runs that measured nothing: failed, or skipped of no group to
    /// tell it in.
    pub unmeasured: Vec<Unmeasured>,
}

/// How a session's runs ended, each of its last end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Runs {
    /// Those that ended well.
    pub done: usize,
    /// Those that failed.
    pub failed: usize,
    /// Those skipped, of a feature that their terminal has not.
    pub skipped: usize,
}

/// A terminal of the session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Terminal {
    /// Its name, as the terminals' file names it.
    pub name: String,
    /// Its version, if its records tell one.
    pub version: Option<String>,
}

/// A metric of a tool, of records of the same conditions.
#[derive(Clone, Debug, Serialize)]
pub struct Group {
    /// The tool.
    pub tool: String,
    /// The metric.
    pub metric: String,
    /// Its unit.
    pub unit: String,
    /// Its title.
    pub title: String,
    /// Which way is better, if one.
    pub better: Option<Better>,
    /// The feature that its records measured, if one.
    pub feature: Option<String>,
    /// Its records' conditions.
    pub conditions: Map<String, Value>,
    /// The conditions that tell it from the tool's other groups of the
    /// same metric, as `text` of the throughput's.
    pub telling: Vec<String>,
    /// Each terminal's samples summed up.
    pub rows: Vec<Row>,
    /// The terminals that the coordinator skipped, of the feature.
    pub skipped: Vec<Skip>,
    /// The A/B's change, if the group holds both.
    pub change: Option<AbChange>,
}

/// A terminal's samples of a group.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    /// The terminal.
    pub terminal: String,
    /// Their summary.
    pub summary: Summary,
    /// The median of each cycle, of more than one.
    pub cycles: Vec<CycleMedian>,
    /// The samples, of every record, in their order.
    #[serde(skip)]
    pub samples: Vec<f64>,
}

/// A cycle's median.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CycleMedian {
    /// The cycle, from 1.
    pub cycle: u32,
    /// Its samples' median.
    pub median: f64,
}

/// A terminal skipped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Skip {
    /// The terminal.
    pub terminal: String,
    /// Why.
    pub why: String,
}

/// The change of a metric's median from the A/B's A to its B.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AbChange {
    /// A's median.
    pub a: f64,
    /// B's median.
    pub b: f64,
    /// B's less A's.
    pub change: f64,
    /// The change in percent of A's, of an A not 0.
    pub percent: Option<f64>,
    /// The 95 % bootstrap interval of the change.
    pub interval: (f64, f64),
    /// What it tells.
    pub verdict: Verdict,
}

/// What a change tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// B is better beyond the noise.
    Better,
    /// B is worse beyond the noise.
    Worse,
    /// B is higher beyond the noise, of a metric of no better way.
    Higher,
    /// B is lower beyond the noise, of a metric of no better way.
    Lower,
    /// The interval holds no change.
    WithinNoise,
    /// Too few samples to tell the noise, less than two of either.
    TooFew,
}

/// A budget judged, of a group and a terminal, or of the A/B's change.
#[derive(Clone, Debug, Serialize)]
pub struct Judgement {
    /// The budget's place in the session's file, from 1.
    pub budget: usize,
    /// The budget's metric.
    pub metric: String,
    /// The statistic.
    pub stat: Stat,
    /// The group that it judged, its place in the report's, if one.
    pub group: Option<usize>,
    /// The terminal, or none of the A/B's change.
    pub terminal: Option<String>,
    /// The limits, as a text.
    pub limit: String,
    /// The statistic, or its change from A to B, in the metric's unit.
    pub value: Option<f64>,
    /// The change in percent of A's statistic, of a change.
    pub percent: Option<f64>,
    /// The 95 % bootstrap interval of the change, of a change.
    pub interval: Option<(f64, f64)>,
    /// Whether it held; none where nothing was there to judge.
    pub held: Option<bool>,
    /// Why nothing was there to judge.
    pub note: Option<String>,
}

/// A run that measured nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Unmeasured {
    /// Its id.
    pub id: String,
    /// Its tool.
    pub tool: String,
    /// Its terminal.
    pub terminal: String,
    /// Its cycle.
    pub cycle: u32,
    /// Whether it failed, or was skipped.
    pub status: Status,
    /// Why.
    pub why: String,
}

/// The report of `input`.
pub fn report(input: &Input<'_>) -> Report {
    let order = terminal_order(input.terminals, input.records);
    let mut groups = groups(input.records, &order);
    let last = results::last_of_each(input.log);
    let unmeasured = mark_skipped(&mut groups, &last);
    if let Some(ab) = &input.ab {
        for group in &mut groups {
            group.change = ab_change(group, ab);
        }
    }
    let judgements = input
        .budgets
        .iter()
        .enumerate()
        .flat_map(|(at, budget)| judge(at + 1, budget, &groups, input.ab.as_ref()))
        .collect();
    let terminals = order
        .iter()
        .map(|name| Terminal {
            name: name.clone(),
            version: input
                .records
                .iter()
                .filter(|record| &record.terminal == name)
                .find_map(|record| record.version.clone()),
        })
        .collect();
    Report {
        session: input.session.clone(),
        machine: input.machine.clone(),
        span: span(input.log),
        runs: (!last.is_empty()).then(|| Runs {
            done: count(&last, Status::Done),
            failed: count(&last, Status::Failed),
            skipped: count(&last, Status::Skipped),
        }),
        terminals,
        ab: input.ab.clone(),
        groups,
        judgements,
        unmeasured,
    }
}

/// The terminals of `session`, then those of `records` that it lacks, in
/// their order of appearance.
fn terminal_order(session: &[String], records: &[Record]) -> Vec<String> {
    let mut order: Vec<String> = session.to_vec();
    for record in records {
        if !order.contains(&record.terminal) {
            order.push(record.terminal.clone());
        }
    }
    order
}

/// The runs of `last` that ended of `status`.
fn count(last: &[&Entry], status: Status) -> usize {
    last.iter().filter(|entry| entry.status == status).count()
}

/// When the log's first run started and its last ended.
fn span(log: &[Entry]) -> Option<(u64, u64)> {
    let from = log.iter().map(|entry| entry.started).min()?;
    let to = log
        .iter()
        .map(|entry| entry.started + entry.seconds.max(0.0).ceil() as u64)
        .max()?;
    Some((from, to))
}

/// A group's key: its tool, metric, unit and conditions, each condition
/// as its JSON text.
type Key = (String, String, String, BTreeMap<String, String>);

/// The groups of `records`, in their order of appearance, each of the
/// terminals in `order`.
fn groups(records: &[Record], order: &[String]) -> Vec<Group> {
    let mut keys: Vec<Key> = Vec::new();
    let mut members: BTreeMap<Key, Vec<&Record>> = BTreeMap::new();
    for record in records {
        let key = (
            record.tool.clone(),
            record.metric.clone(),
            record.unit.clone(),
            record
                .conditions
                .iter()
                .map(|(name, value)| (name.clone(), value.to_string()))
                .collect(),
        );
        if !members.contains_key(&key) {
            keys.push(key.clone());
        }
        members.entry(key).or_default().push(record);
    }
    let groups: Vec<Group> = keys
        .iter()
        .map(|key| {
            let records = &members[key];
            let (title, better) = metrics::of(&key.0, &key.1);
            let conditions = records[0].conditions.clone();
            Group {
                tool: key.0.clone(),
                metric: key.1.clone(),
                unit: key.2.clone(),
                title,
                better,
                feature: conditions
                    .get("feature")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                conditions,
                telling: Vec::new(),
                rows: order
                    .iter()
                    .filter_map(|terminal| row(terminal, records))
                    .collect(),
                skipped: Vec::new(),
                change: None,
            }
        })
        .collect();
    // A tool's groups of one metric side by side, in their order of
    // appearance.
    let rank = |group: &Group| {
        let tool = groups.iter().position(|other| other.tool == group.tool);
        let metric = groups
            .iter()
            .position(|other| (&other.tool, &other.metric) == (&group.tool, &group.metric));
        (tool, metric)
    };
    let ranks: Vec<_> = groups.iter().map(rank).collect();
    let mut ranked: Vec<(_, Group)> = ranks.into_iter().zip(groups).collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    let mut groups: Vec<Group> = ranked.into_iter().map(|(_, group)| group).collect();
    // The conditions that differ between a tool's groups of one metric.
    for at in 0..groups.len() {
        let mut telling = BTreeSet::new();
        for other in &groups {
            if (&other.tool, &other.metric) != (&groups[at].tool, &groups[at].metric) {
                continue;
            }
            let names: BTreeSet<&String> = groups[at]
                .conditions
                .keys()
                .chain(other.conditions.keys())
                .collect();
            for name in names {
                if name != "feature"
                    && groups[at].conditions.get(name) != other.conditions.get(name)
                {
                    telling.insert(name.clone());
                }
            }
        }
        groups[at].telling = telling.into_iter().collect();
    }
    groups
}

/// The row of `terminal`'s samples of `records`, if it has any.
fn row(terminal: &str, records: &[&Record]) -> Option<Row> {
    let own: Vec<&&Record> = records
        .iter()
        .filter(|record| record.terminal == terminal)
        .collect();
    let samples: Vec<f64> = own
        .iter()
        .flat_map(|record| record.samples.iter().copied())
        .collect();
    let summary = stats::summary(&samples)?;
    let mut by_cycle: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for record in &own {
        if let Some(cycle) = record.cycle {
            by_cycle
                .entry(cycle)
                .or_default()
                .extend(record.samples.iter().copied());
        }
    }
    let cycles = if by_cycle.len() > 1 {
        by_cycle
            .into_iter()
            .filter_map(|(cycle, samples)| {
                stats::summary(&samples).map(|summary| CycleMedian {
                    cycle,
                    median: summary.median,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(Row {
        terminal: terminal.to_owned(),
        summary,
        cycles,
        samples,
    })
}

/// Marks in `groups` the terminals of `last`'s skipped runs, of the
/// groups of their tool and feature: the runs that measured nothing and
/// are told in no group.
fn mark_skipped(groups: &mut [Group], last: &[&Entry]) -> Vec<Unmeasured> {
    let mut unmeasured = Vec::new();
    for entry in last {
        let why = entry.error.clone().unwrap_or_default();
        match entry.status {
            Status::Done => {}
            Status::Failed => unmeasured.push(Unmeasured {
                id: entry.id.clone(),
                tool: entry.tool.clone(),
                terminal: entry.terminal.clone(),
                cycle: entry.cycle,
                status: entry.status,
                why,
            }),
            Status::Skipped => {
                let mut told = false;
                for group in groups.iter_mut().filter(|group| {
                    group.tool == entry.tool && group.feature.as_deref() == entry.feature()
                }) {
                    told = true;
                    let known = group.rows.iter().any(|row| row.terminal == entry.terminal)
                        || group
                            .skipped
                            .iter()
                            .any(|skip| skip.terminal == entry.terminal);
                    if !known {
                        group.skipped.push(Skip {
                            terminal: entry.terminal.clone(),
                            why: why.clone(),
                        });
                    }
                }
                if !told
                    && !unmeasured.iter().any(|other: &Unmeasured| {
                        other.status == Status::Skipped
                            && (&other.tool, &other.terminal) == (&entry.tool, &entry.terminal)
                    })
                {
                    unmeasured.push(Unmeasured {
                        id: entry.id.clone(),
                        tool: entry.tool.clone(),
                        terminal: entry.terminal.clone(),
                        cycle: entry.cycle,
                        status: entry.status,
                        why,
                    });
                }
            }
        }
    }
    unmeasured
}

/// The least samples of each side of an A/B that tell its noise.
const LEAST_SAMPLES: usize = 2;

/// The A/B's change of `group`'s median, if it holds both terminals.
fn ab_change(group: &Group, ab: &Ab) -> Option<AbChange> {
    let a = group.rows.iter().find(|row| row.terminal == ab.a)?;
    let b = group.rows.iter().find(|row| row.terminal == ab.b)?;
    let (of_a, of_b) = (a.summary.median, b.summary.median);
    let change = of_b - of_a;
    let interval = stats::change_interval(&a.samples, &b.samples, Stat::Median)?;
    let verdict = if a.summary.count < LEAST_SAMPLES || b.summary.count < LEAST_SAMPLES {
        Verdict::TooFew
    } else if interval.0 > 0.0 || interval.1 < 0.0 {
        match (group.better, change > 0.0) {
            (Some(Better::Lower), true) | (Some(Better::Higher), false) => Verdict::Worse,
            (Some(Better::Lower), false) | (Some(Better::Higher), true) => Verdict::Better,
            (None, true) => Verdict::Higher,
            (None, false) => Verdict::Lower,
        }
    } else {
        Verdict::WithinNoise
    };
    Some(AbChange {
        a: of_a,
        b: of_b,
        change,
        percent: (of_a != 0.0).then(|| change / of_a.abs() * 100.0),
        interval,
        verdict,
    })
}

/// The judgements of `budget`, the `place`th of the session's file, of
/// `groups`.
fn judge(place: usize, budget: &Budget, groups: &[Group], ab: Option<&Ab>) -> Vec<Judgement> {
    let blank = |group: Option<usize>, terminal: Option<String>, limit: String| Judgement {
        budget: place,
        metric: budget.metric.clone(),
        stat: budget.stat,
        group,
        terminal,
        limit,
        value: None,
        percent: None,
        interval: None,
        held: None,
        note: None,
    };
    let mut judgements = Vec::new();
    for (at, group) in groups.iter().enumerate() {
        if !budget.judges(&group.tool, &group.metric, &group.conditions) {
            continue;
        }
        if budget.limits() {
            let limit = budget.describe_limits(&group.unit);
            let mut any = false;
            for row in &group.rows {
                if !budget.judges_terminal(&row.terminal) {
                    continue;
                }
                any = true;
                let value = budget.stat.of(&row.summary);
                judgements.push(Judgement {
                    value: Some(value),
                    held: Some(budget.holds(value)),
                    ..blank(Some(at), Some(row.terminal.clone()), limit.clone())
                });
            }
            if !any {
                judgements.push(Judgement {
                    note: Some(budget.terminal.as_ref().map_or_else(
                        || "no terminal's record".to_owned(),
                        |terminal| format!("no record of {terminal}"),
                    )),
                    ..blank(Some(at), budget.terminal.clone(), limit)
                });
            }
        }
        if budget.changes()
            && let Some(ab) = ab
        {
            let limit = budget.describe_changes(&group.unit);
            let a = group.rows.iter().find(|row| row.terminal == ab.a);
            let b = group.rows.iter().find(|row| row.terminal == ab.b);
            let judgement = match (a, b) {
                (Some(a), Some(b)) => {
                    let (of_a, of_b) = (budget.stat.of(&a.summary), budget.stat.of(&b.summary));
                    let held = budget.change_holds(of_a, of_b);
                    Judgement {
                        value: Some(of_b - of_a),
                        percent: (of_a != 0.0).then(|| (of_b - of_a) / of_a.abs() * 100.0),
                        interval: stats::change_interval(&a.samples, &b.samples, budget.stat),
                        held,
                        note: held.is_none().then(|| {
                            format!("{}'s {} is 0, of no share", ab.a, budget.stat.name())
                        }),
                        ..blank(Some(at), None, limit)
                    }
                }
                (a, _) => {
                    let missing = if a.is_none() { &ab.a } else { &ab.b };
                    Judgement {
                        note: Some(format!("no record of {missing}")),
                        ..blank(Some(at), None, limit)
                    }
                }
            };
            judgements.push(judgement);
        }
    }
    if judgements.is_empty() {
        let mut limit = budget.describe_limits("");
        if budget.changes() {
            if !limit.is_empty() {
                limit.push_str("; ");
            }
            limit.push_str(&budget.describe_changes(""));
        }
        judgements.push(Judgement {
            note: Some("no record of it".to_owned()),
            ..blank(None, budget.terminal.clone(), limit.trim().to_owned())
        });
    }
    judgements
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttybench_core::budget::Change;

    fn record(tool: &str, terminal: &str, metric: &str, samples: &[f64], cycle: u32) -> Record {
        Record {
            tool: tool.to_owned(),
            terminal: terminal.to_owned(),
            version: Some(format!("{terminal}-1")),
            metric: metric.to_owned(),
            unit: "ms".to_owned(),
            samples: samples.to_vec(),
            conditions: Map::new(),
            cycle: Some(cycle),
        }
    }

    fn of(mut record: Record, conditions: &[(&str, Value)]) -> Record {
        for (name, value) in conditions {
            record.conditions.insert((*name).to_owned(), value.clone());
        }
        record
    }

    fn entry(id: &str, tool: &str, terminal: &str, args: &[&str], status: Status) -> Entry {
        Entry {
            id: id.to_owned(),
            cycle: 1,
            run: 1,
            position: 1,
            tool: tool.to_owned(),
            terminal: terminal.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            started: 1_000,
            seconds: 12.5,
            status,
            code: None,
            error: Some(format!("{id} why")),
        }
    }

    fn budget(text: &str) -> Budget {
        #[derive(serde::Deserialize)]
        struct One {
            budget: Budget,
        }
        let one: One = ttybench_core::config::parse(std::path::Path::new("b.toml"), text).unwrap();
        one.budget
    }

    fn input<'a>(
        records: &'a [Record],
        log: &'a [Entry],
        terminals: &'a [String],
        ab: Option<Ab>,
        budgets: &'a [Budget],
    ) -> Input<'a> {
        Input {
            session: "s".to_owned(),
            machine: None,
            records,
            log,
            terminals,
            ab,
            budgets,
        }
    }

    #[test]
    fn records_of_the_same_conditions_alone_go_together() {
        let records = [
            record("startup", "b", "ready_ms", &[10.0, 12.0], 1),
            record("startup", "a", "ready_ms", &[20.0, 22.0], 1),
            record("startup", "a", "ready_ms", &[30.0, 32.0], 2),
            of(
                record("throughput", "a", "written_mib_s", &[300.0], 1),
                &[("text", "plain".into()), ("mib", 256.into())],
            ),
            of(
                record("throughput", "a", "written_mib_s", &[200.0], 1),
                &[("text", "colors".into()), ("mib", 256.into())],
            ),
            of(
                record("scroll", "a", "frames_per_s", &[60.0, 61.0], 1),
                &[("feature", "smooth".into())],
            ),
            record("scroll", "b", "frames_per_s", &[20.0, 21.0], 1),
        ];
        let terminals = ["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let log = [
            entry("c1-r1-startup-a", "startup", "a", &[], Status::Done),
            entry(
                "c1-r2-scroll-b",
                "scroll",
                "b",
                &["--feature", "smooth"],
                Status::Skipped,
            ),
            entry(
                "c1-r2-scroll-c",
                "scroll",
                "c",
                &["--feature", "smooth"],
                Status::Skipped,
            ),
            entry(
                "c1-r3-memory-c",
                "memory",
                "c",
                &["--feature", "x"],
                Status::Skipped,
            ),
            entry("c1-r4-idle-c", "idle", "c", &[], Status::Failed),
            entry("c1-r5-idle-b", "idle", "b", &[], Status::Failed),
            entry("c1-r5-idle-b", "idle", "b", &[], Status::Done),
        ];
        let report = report(&input(&records, &log, &terminals, None, &[]));
        let names: Vec<(&str, &str, Vec<&str>)> = report
            .groups
            .iter()
            .map(|group| {
                (
                    group.tool.as_str(),
                    group.metric.as_str(),
                    group.rows.iter().map(|row| row.terminal.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("startup", "ready_ms", vec!["a", "b"]),
                ("throughput", "written_mib_s", vec!["a"]),
                ("throughput", "written_mib_s", vec!["a"]),
                ("scroll", "frames_per_s", vec!["a"]),
                ("scroll", "frames_per_s", vec!["b"]),
            ]
        );
        let startup = &report.groups[0];
        assert_eq!(startup.title, "From the launch to running the payload");
        assert_eq!(startup.rows[0].summary.count, 4);
        assert_eq!(
            startup.rows[0].cycles,
            [
                CycleMedian {
                    cycle: 1,
                    median: 21.0
                },
                CycleMedian {
                    cycle: 2,
                    median: 31.0
                }
            ]
        );
        assert!(startup.rows[1].cycles.is_empty());
        assert!(startup.telling.is_empty());
        assert_eq!(report.groups[1].telling, ["text"]);
        // The feature tells the scroll's groups apart, of no telling
        // condition of its own; its skipped terminals in its group alone.
        let smooth = &report.groups[3];
        assert_eq!(smooth.feature.as_deref(), Some("smooth"));
        assert!(smooth.telling.is_empty());
        let skipped: Vec<&str> = smooth.skipped.iter().map(|s| s.terminal.as_str()).collect();
        assert_eq!(skipped, ["b", "c"]);
        assert!(report.groups[4].skipped.is_empty());
        // A failed run, and a skipped one of no group, measured nothing; a
        // failed run done again did.
        let unmeasured: Vec<(&str, Status)> = report
            .unmeasured
            .iter()
            .map(|run| (run.id.as_str(), run.status))
            .collect();
        assert_eq!(
            unmeasured,
            [
                ("c1-r3-memory-c", Status::Skipped),
                ("c1-r4-idle-c", Status::Failed)
            ]
        );
        assert_eq!(
            report.runs,
            Some(Runs {
                done: 2,
                failed: 1,
                skipped: 3
            })
        );
        assert_eq!(report.span, Some((1_000, 1_013)));
        let versions: Vec<(&str, Option<&str>)> = report
            .terminals
            .iter()
            .map(|t| (t.name.as_str(), t.version.as_deref()))
            .collect();
        assert_eq!(
            versions,
            [("a", Some("a-1")), ("b", Some("b-1")), ("c", None)]
        );
    }

    #[test]
    fn a_tools_groups_of_one_metric_side_by_side() {
        let smooth = [("feature", Value::from("smooth"))];
        let records = [
            of(record("scroll", "a", "frames_per_s", &[60.0], 1), &smooth),
            of(record("scroll", "a", "frame_gap_ms", &[16.0], 1), &smooth),
            record("startup", "a", "ready_ms", &[200.0], 1),
            record("scroll", "a", "frames_per_s", &[20.0], 1),
            record("scroll", "a", "frame_gap_ms", &[50.0], 1),
        ];
        let report = report(&input(&records, &[], &[], None, &[]));
        let order: Vec<(&str, Option<&str>)> = report
            .groups
            .iter()
            .map(|group| (group.metric.as_str(), group.feature.as_deref()))
            .collect();
        assert_eq!(
            order,
            [
                ("frames_per_s", Some("smooth")),
                ("frames_per_s", None),
                ("frame_gap_ms", Some("smooth")),
                ("frame_gap_ms", None),
                ("ready_ms", None),
            ]
        );
    }

    #[test]
    fn an_ab_tells_a_change_beyond_its_noise() {
        let slow: Vec<f64> = (0..20).map(|at| 250.0 + f64::from(at % 4)).collect();
        let fast: Vec<f64> = slow.iter().map(|value| value - 20.0).collect();
        let noisy: Vec<f64> = (0..20).map(|at| 100.0 + f64::from(at % 7)).collect();
        let records = [
            record("startup", "old", "ready_ms", &slow, 1),
            record("startup", "new", "ready_ms", &fast, 1),
            record("throughput", "old", "written_mib_s", &slow, 1),
            record("throughput", "new", "written_mib_s", &fast, 1),
            record("memory", "old", "footprint_mib", &noisy, 1),
            record("memory", "new", "footprint_mib", &noisy, 1),
            record("frames", "old", "frames_per_s", &slow, 1),
            record("frames", "new", "frames_per_s", &fast, 1),
            record("motion", "old", "missed_frames", &[3.0], 1),
            record("motion", "new", "missed_frames", &[9.0], 1),
            record("idle", "old", "cpu_percent", &[0.1, 0.2], 1),
        ];
        let ab = Ab {
            a: "old".to_owned(),
            b: "new".to_owned(),
        };
        let report = report(&input(&records, &[], &[], Some(ab), &[]));
        let verdicts: Vec<(&str, Option<Verdict>)> = report
            .groups
            .iter()
            .map(|group| {
                (
                    group.tool.as_str(),
                    group.change.map(|change| change.verdict),
                )
            })
            .collect();
        assert_eq!(
            verdicts,
            [
                ("startup", Some(Verdict::Better)),
                ("throughput", Some(Verdict::Worse)),
                ("memory", Some(Verdict::WithinNoise)),
                ("frames", Some(Verdict::Lower)),
                ("motion", Some(Verdict::TooFew)),
                ("idle", None),
            ]
        );
        let change = report.groups[0].change.unwrap();
        assert!((change.change + 20.0).abs() < 1e-9);
        assert!((change.percent.unwrap() + 20.0 / change.a * 100.0).abs() < 1e-9);
        assert!(change.interval.1 < 0.0);
    }

    /// A judgement's budget, terminal, value, whether it held, and its
    /// note.
    type Judged<'a> = (
        usize,
        Option<&'a str>,
        Option<f64>,
        Option<bool>,
        Option<&'a str>,
    );

    #[test]
    fn budgets_judged_of_each_terminal_and_of_the_change() {
        let records = [
            record("startup", "old", "ready_ms", &[100.0, 110.0, 120.0], 1),
            record("startup", "new", "ready_ms", &[130.0, 140.0, 150.0], 1),
            of(
                record("throughput", "old", "written_mib_s", &[300.0, 310.0], 1),
                &[("text", "plain".into())],
            ),
            of(
                record("throughput", "new", "written_mib_s", &[290.0, 300.0], 1),
                &[("text", "plain".into())],
            ),
            of(
                record("throughput", "old", "written_mib_s", &[0.0, 0.0], 1),
                &[("text", "colors".into())],
            ),
            of(
                record("throughput", "new", "written_mib_s", &[1.0, 1.0], 1),
                &[("text", "colors".into())],
            ),
        ];
        let budgets = [
            budget("[budget]\ntool = \"startup\"\nmetric = \"ready_ms\"\nat-most = 125\n"),
            budget(
                "[budget]\nmetric = \"ready_ms\"\nterminal = \"new\"\nstat = \"max\"\nat-least = 150\n",
            ),
            budget("[budget]\nmetric = \"written_mib_s\"\nchange-at-least = \"-5%\"\n"),
            budget("[budget]\nmetric = \"memory_mib\"\nat-most = 1\n"),
            budget(
                "[budget]\nmetric = \"ready_ms\"\nconditions = { text = \"plain\" }\nat-most = 1\n",
            ),
        ];
        assert_eq!(budgets[2].change_at_least, Some(Change::Percent(-5.0)));
        let ab = Ab {
            a: "old".to_owned(),
            b: "new".to_owned(),
        };
        let report = report(&input(&records, &[], &[], Some(ab), &budgets));
        let judged: Vec<Judged<'_>> = report
            .judgements
            .iter()
            .map(|judgement| {
                (
                    judgement.budget,
                    judgement.terminal.as_deref(),
                    judgement.value,
                    judgement.held,
                    judgement.note.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            judged,
            [
                (1, Some("old"), Some(110.0), Some(true), None),
                (1, Some("new"), Some(140.0), Some(false), None),
                (2, Some("new"), Some(150.0), Some(true), None),
                (3, None, Some(-10.0), Some(true), None),
                (
                    3,
                    None,
                    Some(1.0),
                    None,
                    Some("old's median is 0, of no share")
                ),
                (4, None, None, None, Some("no record of it")),
                (5, None, None, None, Some("no record of it")),
            ]
        );
        let throughput = &report.judgements[3];
        assert!((throughput.percent.unwrap() + 10.0 / 305.0 * 100.0).abs() < 1e-9);
        assert!(throughput.interval.is_some());
        assert_eq!(throughput.limit, "≥ -5 %");
        assert_eq!(report.judgements[0].limit, "≤ 125 ms");
    }
}
