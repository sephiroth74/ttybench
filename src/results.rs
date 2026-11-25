//! What the tools write: a record of each measure, a line of JSON each, a
//! session's folder of them and of its machine, and the coordinator's log
//! of its runs, so that the coordinator, the reports and the charts read
//! every tool alike, and someone else's tool joins them.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// The file of a session's records, in its folder.
pub const RECORDS: &str = "records.jsonl";

/// The file of a session's machine, in its folder.
pub const MACHINE: &str = "machine.json";

/// The file of the coordinator's log of a session's runs, in its folder.
pub const LOG: &str = "runs.jsonl";

/// The folder of a session's runs, a folder of each in it.
pub const RUNS: &str = "runs";

/// A measure of a terminal: the tool that took it, the terminal and its
/// version, the metric, its unit and its samples, and what it was taken
/// of, as a workload or a scenario.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The tool, as `startup`.
    pub tool: String,
    /// The terminal, as its config names it.
    pub terminal: String,
    /// Its version, as its bundle tells it, of its build after it where
    /// that differs, as `1.4.0 (202611220721)`, if one.
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

/// The version of the app at `app`, its bundle's
/// `CFBundleShortVersionString`, of its build, `CFBundleVersion`, after it
/// where the bundle tells one apart, as `1.4.0 (202611220721)`, so that two
/// builds of one version tell apart too; none if it tells no version.
pub fn app_version(app: &Path) -> Option<String> {
    let version = bundle_key(app, "CFBundleShortVersionString")?;
    Some(match bundle_key(app, "CFBundleVersion") {
        Some(build) if build != version => format!("{version} ({build})"),
        _ => version,
    })
}

/// The text of the key `key` of the bundle at `app`'s `Info.plist`, if it
/// holds one.
fn bundle_key(app: &Path, key: &str) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// A run's end, as the coordinator's log holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Its step's id, which names its folder.
    pub id: String,
    /// Its cycle, from 1.
    pub cycle: u32,
    /// Its run of the session's file, from 1.
    pub run: usize,
    /// Its terminal's place in its run's order of the cycle, from 1.
    pub position: usize,
    /// The tool.
    pub tool: String,
    /// The terminal.
    pub terminal: String,
    /// The tool's arguments, but the coordinator's.
    pub args: Vec<String>,
    /// When it started, in seconds since 1970.
    pub started: u64,
    /// How long it took, in seconds.
    pub seconds: f64,
    /// How it ended.
    pub status: Status,
    /// The tool's exit code, if it exited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    /// Why it failed: the last lines of its output, or the coordinator's
    /// reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The machine's load around it, if the coordinator read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load: Option<Load>,
}

/// The machine's load around a run, which tells a run measured beside
/// another program's work, as a scan of the files' or an update's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Load {
    /// The load average of a minute as it started.
    pub before: f64,
    /// The load average of a minute as it ended.
    pub after: f64,
    /// The busiest processes as it ended, the most first, of their share
    /// of a core over about the last minute.
    pub busiest: Vec<Busy>,
}

/// A process's share of a core.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Busy {
    /// Its executable's name.
    pub name: String,
    /// Its share of a core, in percent.
    pub cpu_percent: f64,
}

impl Load {
    /// The load as a run starts: its average of a minute, of none read as
    /// none.
    pub fn starting() -> f64 {
        load_average().unwrap_or(f64::NAN)
    }

    /// The load as a run that started of `before` ends.
    pub fn ending(before: f64) -> Self {
        Self {
            before,
            after: load_average().unwrap_or(f64::NAN),
            busiest: busiest(BUSIEST, BUSY_PERCENT),
        }
    }
}

/// How many of the busiest processes a run's load tells.
const BUSIEST: usize = 3;

/// The least share of a core of a process that a run's load tells.
const BUSY_PERCENT: f64 = 10.0;

/// The machine's load average of a minute.
pub fn load_average() -> Option<f64> {
    let mut loads = [0.0f64; 3];
    // SAFETY: `loads` holds the three values that `getloadavg` writes at
    // most.
    let read = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
    (read >= 1).then_some(loads[0])
}

/// The `count` busiest processes of `least` percent of a core at least,
/// the most first, of `ps`'s share of a core, a decaying average of about
/// a minute, which tells every user's processes of no privilege.
pub fn busiest(count: usize, least: f64) -> Vec<Busy> {
    let Ok(output) = Command::new("/bin/ps")
        .args(["-A", "-o", "%cpu=,comm="])
        .output()
    else {
        return Vec::new();
    };
    let mut busy: Vec<Busy> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (cpu, command) = line.trim_start().split_once(' ')?;
            let name = command.trim().rsplit('/').next()?.to_owned();
            Some(Busy {
                name,
                cpu_percent: cpu.parse().ok()?,
            })
        })
        .filter(|busy| busy.cpu_percent >= least)
        .collect();
    busy.sort_by(|a, b| b.cpu_percent.total_cmp(&a.cpu_percent));
    busy.truncate(count);
    busy
}

impl Entry {
    /// The feature that its run measured, of its `--feature`, if one.
    pub fn feature(&self) -> Option<&str> {
        self.args
            .iter()
            .position(|arg| arg == "--feature")
            .and_then(|at| self.args.get(at + 1))
            .map(String::as_str)
    }
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Its tool exited of 0.
    Done,
    /// Its tool exited of another code, did not start, or took too long.
    Failed,
    /// Its tool measured nothing, as the terminal has not the feature that
    /// the run names, which would measure another thing.
    Skipped,
}

/// The entries of the log of the session in `folder`, in their order;
/// none of no log.
pub fn read_log(folder: &Path) -> io::Result<Vec<Entry>> {
    let path = folder.join(LOG);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut entries = Vec::new();
    for (at, line) in io::BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        entries.push(serde_json::from_str(&line).map_err(|error| {
            io::Error::other(format!("{}:{}: {error}", path.display(), at + 1))
        })?);
    }
    Ok(entries)
}

/// The last entry of each run of `entries`, by its id, in the order of
/// those last entries: how each run ended at last, as `--resume` runs a
/// failed one again.
pub fn last_of_each(entries: &[Entry]) -> Vec<&Entry> {
    let last: std::collections::BTreeMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(at, entry)| (entry.id.as_str(), at))
        .collect();
    entries
        .iter()
        .enumerate()
        .filter(|(at, entry)| last.get(entry.id.as_str()) == Some(at))
        .map(|(_, entry)| entry)
        .collect()
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
    fn the_machines_load() {
        let load = load_average().unwrap();
        assert!(load.is_finite() && load >= 0.0, "{load}");
        // ps tells this test's process among the others; the busiest are
        // sorted, each of its name alone.
        let busy = busiest(1000, 0.0);
        assert!(!busy.is_empty());
        assert!(
            busy.windows(2)
                .all(|pair| pair[0].cpu_percent >= pair[1].cpu_percent)
        );
        assert!(busy.iter().all(|busy| !busy.name.contains('/')), "{busy:?}");
        let ended = Load::ending(1.5);
        assert_eq!(ended.before, 1.5);
        assert!(ended.busiest.len() <= BUSIEST);
    }

    #[test]
    fn an_app_tells_its_version() {
        let version =
            app_version(Path::new("/System/Applications/Utilities/Terminal.app")).unwrap();
        assert!(version.contains('.'), "{version}");
        // A build that is not the version goes after it; one that is, not.
        let folder = std::env::temp_dir().join(format!("ttybench-bundle-{}", std::process::id()));
        for (build, told) in [("456", "1.2.3 (456)"), ("1.2.3", "1.2.3")] {
            let contents = folder.join("X.app/Contents");
            std::fs::create_dir_all(&contents).unwrap();
            std::fs::write(
                contents.join("Info.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleShortVersionString</key><string>1.2.3</string><key>CFBundleVersion</key><string>{build}</string></dict></plist>\n"
                ),
            )
            .unwrap();
            assert_eq!(app_version(&folder.join("X.app")).as_deref(), Some(told));
        }
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(app_version(Path::new("/nonexistent.app")), None);
    }
}
