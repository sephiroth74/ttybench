//! The session's log of its runs, `runs.jsonl`, a line as each ends, and
//! its records, gathered anew after each run of the runs that ended well,
//! so that a session cut off anywhere holds each record once.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::Path;

use ttybench_core::results::{self, RECORDS};

pub use ttybench_core::results::{Entry, LOG, RUNS, Status};

/// The entries of the log of the session in `folder`, in their order;
/// none of no log.
pub fn read(folder: &Path) -> io::Result<Vec<Entry>> {
    results::read_log(folder)
}

/// Appends `entry` to the log of the session in `folder`.
pub fn append(folder: &Path, entry: &Entry) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join(LOG))?;
    let line = serde_json::to_string(entry).map_err(io::Error::other)?;
    writeln!(file, "{line}")?;
    file.sync_data()
}

/// The ids of `entries` whose last entry ended: done, or skipped, as a
/// run of a feature that its terminal has not stays so.
pub fn done(entries: &[Entry]) -> BTreeSet<String> {
    results::last_of_each(entries)
        .into_iter()
        .filter(|entry| entry.status != Status::Failed)
        .map(|entry| entry.id.clone())
        .collect()
}

/// Writes the records of the session in `folder` anew, of the runs whose
/// last entry is done, in the order of those entries, each record of its
/// run's cycle: how many it wrote.
pub fn gather(folder: &Path) -> io::Result<usize> {
    let entries = read(folder)?;
    let mut lines = String::new();
    let mut count = 0;
    for entry in results::last_of_each(&entries) {
        if entry.status != Status::Done {
            continue;
        }
        let path = folder.join(RUNS).join(&entry.id).join(RECORDS);
        if !path.exists() {
            continue;
        }
        for mut record in results::read_records(&path)? {
            record.cycle = Some(entry.cycle);
            lines.push_str(&serde_json::to_string(&record).map_err(io::Error::other)?);
            lines.push('\n');
            count += 1;
        }
    }
    let temporary = folder.join(format!("{RECORDS}.new"));
    std::fs::write(&temporary, lines)?;
    std::fs::rename(&temporary, folder.join(RECORDS))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttybench_core::results::{Record, Session};

    fn entry(id: &str, cycle: u32, status: Status) -> Entry {
        Entry {
            id: id.to_owned(),
            cycle,
            run: 1,
            position: 1,
            tool: "startup".to_owned(),
            terminal: "a".to_owned(),
            args: Vec::new(),
            started: 0,
            seconds: 1.0,
            status,
            code: Some(i32::from(status == Status::Failed)),
            error: None,
        }
    }

    fn record(metric: &str) -> Record {
        Record {
            tool: "startup".to_owned(),
            terminal: "a".to_owned(),
            version: None,
            metric: metric.to_owned(),
            unit: "ms".to_owned(),
            samples: vec![1.0],
            conditions: serde_json::Map::new(),
            cycle: None,
        }
    }

    #[test]
    fn the_records_of_the_runs_that_ended_well_once_each() {
        let folder = std::env::temp_dir().join(format!("ttybench-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        for (id, metric) in [("one", "first"), ("two", "second"), ("three", "third")] {
            let run = Session::open(&folder.join(RUNS).join(id)).unwrap();
            run.append(&record(metric)).unwrap();
        }
        assert!(read(&folder).unwrap().is_empty());
        append(&folder, &entry("two", 1, Status::Failed)).unwrap();
        append(&folder, &entry("one", 1, Status::Done)).unwrap();
        append(&folder, &entry("three", 2, Status::Failed)).unwrap();
        append(&folder, &entry("two", 1, Status::Done)).unwrap();
        append(&folder, &entry("four", 2, Status::Skipped)).unwrap();
        let entries = read(&folder).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(
            done(&entries),
            ["four", "one", "two"].map(String::from).into()
        );
        for _ in 0..2 {
            assert_eq!(gather(&folder).unwrap(), 2);
        }
        let records = results::read_records(&folder.join(RECORDS)).unwrap();
        let metrics: Vec<(&str, Option<u32>)> = records
            .iter()
            .map(|record| (record.metric.as_str(), record.cycle))
            .collect();
        assert_eq!(metrics, [("first", Some(1)), ("second", Some(1))]);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
