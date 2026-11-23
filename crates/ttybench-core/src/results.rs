//! What the tools write: a record of each measure, a line of JSON each, a
//! session's folder of them and of its machine, so that the coordinator,
//! the reports and the charts read every tool alike, and someone else's
//! tool joins them.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// The file of a session's records, in its folder.
pub const RECORDS: &str = "records.jsonl";

/// The file of a session's machine, in its folder.
pub const MACHINE: &str = "machine.json";

/// A measure of a terminal: the tool that took it, the terminal and its
/// version, the metric, its unit and its samples, and what it was taken
/// of, as a workload or a scenario.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The tool, as `startup`.
    pub tool: String,
    /// The terminal, as its config names it.
    pub terminal: String,
    /// Its version, as its bundle tells it, if one.
    #[serde(default)]
    pub version: Option<String>,
    /// The metric, as `ready_ms`.
    pub metric: String,
    /// Its unit, as `ms` or `MiB/s`.
    pub unit: String,
    /// Its samples, each of a run.
    pub samples: Vec<f64>,
    /// What it was taken of, as `{"workload": "cat", "mib": 64}`.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub conditions: serde_json::Map<String, serde_json::Value>,
    /// The cycle of the coordinator's session that took it, from 1, if
    /// the coordinator ran its tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<u32>,
}

/// The machine that a session measured on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    /// Its model, as `Mac16,8`.
    pub model: String,
    /// Its processor, as `Apple M4 Pro`.
    pub cpu: String,
    /// Its system's version, as `27.2`.
    pub macos: String,
    /// The main display's refresh rate, in hertz, if it tells one.
    pub refresh_hz: Option<f64>,
}

impl Machine {
    /// The machine that the tool runs on.
    pub fn this() -> Self {
        Self {
            model: sysctl("hw.model").unwrap_or_default(),
            cpu: sysctl("machdep.cpu.brand_string").unwrap_or_default(),
            macos: sysctl("kern.osproductversion").unwrap_or_default(),
            refresh_hz: refresh_hz(),
        }
    }
}

/// The text of the `sysctl` value `name`.
fn sysctl(name: &str) -> Option<String> {
    let output = Command::new("/usr/sbin/sysctl")
        .arg("-n")
        .arg(name)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The main display's refresh rate, in hertz; none of a display that
/// tells none, as a virtual one's.
fn refresh_hz() -> Option<f64> {
    use objc2_core_graphics::{CGDisplayCopyDisplayMode, CGDisplayMode, CGMainDisplayID};
    let mode = CGDisplayCopyDisplayMode(CGMainDisplayID())?;
    let rate = CGDisplayMode::refresh_rate(Some(&mode));
    (rate > 0.0).then_some(rate)
}

/// The version of the app at `app`, its bundle's `CFBundleShortVersionString`,
/// if it tells one.
pub fn app_version(app: &Path) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|version| !version.is_empty())
}

/// A session's folder, of its records and its machine.
#[derive(Clone, Debug)]
pub struct Session {
    /// The folder.
    pub folder: PathBuf,
}

impl Session {
    /// The session of `folder`, made if it is not there, of the machine
    /// that the tool runs on written once.
    pub fn open(folder: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(folder)?;
        let machine = folder.join(MACHINE);
        if !machine.exists() {
            let text = serde_json::to_string_pretty(&Machine::this()).map_err(io::Error::other)?;
            std::fs::write(machine, text)?;
        }
        Ok(Self {
            folder: folder.to_path_buf(),
        })
    }

    /// Appends `record` to the session's records.
    pub fn append(&self, record: &Record) -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.folder.join(RECORDS))?;
        let line = serde_json::to_string(record).map_err(io::Error::other)?;
        writeln!(file, "{line}")
    }

    /// The session's records, in their order.
    pub fn records(&self) -> io::Result<Vec<Record>> {
        read_records(&self.folder.join(RECORDS))
    }

    /// The session's machine.
    pub fn machine(&self) -> io::Result<Machine> {
        let text = std::fs::read_to_string(self.folder.join(MACHINE))?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }
}

/// The records of the JSON Lines file at `path`, a line that is none
/// failing of its number.
pub fn read_records(path: &Path) -> io::Result<Vec<Record>> {
    let file = std::fs::File::open(path)?;
    let mut records = Vec::new();
    for (at, line) in io::BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str(&line)
            .map_err(|error| io::Error::other(format!("{}:{}: {error}", path.display(), at + 1)))?;
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_keeps_its_records_and_its_machine() {
        let folder = std::env::temp_dir().join(format!("ttybench-results-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let session = Session::open(&folder).unwrap();
        let mut conditions = serde_json::Map::new();
        conditions.insert("workload".into(), "cat".into());
        let record = Record {
            tool: "throughput".into(),
            terminal: "ghostty".into(),
            version: Some("1.2.3".into()),
            metric: "cat_mib_s".into(),
            unit: "MiB/s".into(),
            samples: vec![300.5, 301.0],
            conditions,
            cycle: None,
        };
        session.append(&record).unwrap();
        session.append(&record).unwrap();
        assert_eq!(session.records().unwrap(), [record.clone(), record]);
        let machine = session.machine().unwrap();
        assert!(
            !machine.model.is_empty() && !machine.macos.is_empty(),
            "{machine:?}"
        );
        // A line that is no record fails of its number.
        std::fs::write(folder.join(RECORDS), "{\"tool\": 1}\n").unwrap();
        let error = session.records().unwrap_err();
        assert!(error.to_string().contains(":1:"), "{error}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_app_tells_its_version() {
        let version = app_version(Path::new("/System/Applications/Utilities/Terminal.app"));
        assert!(version.is_some_and(|version| version.contains('.')));
        assert_eq!(app_version(Path::new("/nonexistent.app")), None);
    }
}
