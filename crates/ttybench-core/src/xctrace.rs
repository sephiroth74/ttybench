//! Instruments' recordings, of `xctrace`, Xcode's: a recording of a
//! template while a workload runs, its tables as `xctrace export` writes
//! them, and its removal once read, as a trace takes hundreds of
//! megabytes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

/// The free bytes of the volume that holds `path`.
pub fn free_bytes(path: &Path) -> std::io::Result<u64> {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(std::io::Error::other)?;
    // SAFETY: `stats` is plain data, which `statvfs` fills in.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a path of no NUL, and a writable `statvfs`.
    if unsafe { libc::statvfs(path.as_ptr(), &raw mut stats) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(u64::from(stats.f_bavail) * stats.f_frsize)
}

/// A trace on disk, removed as it drops.
#[derive(Debug)]
pub struct Trace {
    path: PathBuf,
}

impl Trace {
    /// Its path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keeps it on disk, of no removal: its path.
    pub fn keep(self) -> PathBuf {
        let path = self.path.clone();
        std::mem::forget(self);
        path
    }
}

impl Drop for Trace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A recording under way.
#[derive(Debug)]
pub struct Recording {
    child: Child,
    // Removed as the recording drops, unless it finished.
    trace: Option<Trace>,
}

impl Recording {
    /// Starts recording the template `template` of the process `pid` and of
    /// what the template records of the whole system, for `limit`, into
    /// `path`, a trace of its own; once it records, as `xctrace` tells it.
    pub fn start(
        template: &str,
        pid: libc::pid_t,
        limit: Duration,
        path: &Path,
    ) -> Result<Self, String> {
        let _ = std::fs::remove_dir_all(path);
        let mut child = Command::new("/usr/bin/xcrun")
            .args(["xctrace", "record", "--template", template, "--attach"])
            .arg(pid.to_string())
            .arg("--time-limit")
            .arg(format!("{}ms", limit.as_millis()))
            .arg("--output")
            .arg(path)
            .arg("--no-prompt")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("xctrace, of Xcode: {error}"))?;
        let trace = Trace {
            path: path.to_owned(),
        };
        let stdout = child.stdout.take().ok_or("xctrace gave no output")?;
        let (sender, lines) = mpsc::channel();
        // Its output read to its end, past the lines looked for, so that
        // xctrace never writes to a pipe of no reader.
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let _ = sender.send(line);
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            let left = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or("xctrace did not start recording within a minute")?;
            match lines.recv_timeout(left) {
                Ok(line) if line.starts_with("Starting recording") => break,
                Ok(_) => {}
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("xctrace ended before it recorded".into());
                }
            }
        }
        Ok(Self {
            child,
            trace: Some(trace),
        })
    }

    /// Waits for the recording to end and its trace to be saved: the trace.
    pub fn finish(mut self) -> Result<Trace, String> {
        let status = self
            .child
            .wait()
            .map_err(|error| format!("xctrace: {error}"))?;
        let trace = self.trace.take().ok_or("the trace went")?;
        if !status.success() || !trace.path.exists() {
            return Err(format!("xctrace saved no trace: {status}"));
        }
        Ok(trace)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// A cell of a table: its value, and its formatted text, as the export
/// writes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// Its value, as a number of nanoseconds.
    pub text: Option<String>,
    /// Its formatted text, as `ghostty (78543)`.
    pub fmt: Option<String>,
}

impl Cell {
    /// Its value as a whole number.
    pub fn number(&self) -> Option<u64> {
        self.text.as_deref()?.trim().parse().ok()
    }

    /// The process of a cell of a process, `ghostty (78543)`.
    pub fn pid(&self) -> Option<libc::pid_t> {
        let fmt = self.fmt.as_deref()?.trim();
        let open = fmt.rfind('(')?;
        fmt[open + 1..].strip_suffix(')')?.parse().ok()
    }
}

/// A table of a trace: its columns' mnemonics and its rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    /// The columns' mnemonics, as `start`.
    pub columns: Vec<String>,
    /// The rows, a cell a column.
    pub rows: Vec<Vec<Cell>>,
}

impl Table {
    /// The column of the mnemonic `name`, if the table has it.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column == name)
    }
}

/// An element open while a table's text is read.
struct Open {
    id: Option<String>,
    fmt: Option<String>,
    text: String,
    cell: bool,
}

/// The attributes `id`, `ref` and `fmt` of an element.
fn attributes(element: &BytesStart<'_>) -> (Option<String>, Option<String>, Option<String>) {
    let (mut id, mut reference, mut fmt) = (None, None, None);
    for attribute in element.attributes().flatten() {
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map(std::borrow::Cow::into_owned)
            .ok();
        match attribute.key.as_ref() {
            "id" => id = value,
            "ref" => reference = value,
            "fmt" => fmt = value,
            _ => {}
        }
    }
    (id, reference, fmt)
}

/// The table of an export's text: of the export's references resolved, a
/// value written once and named again by its `id`.
pub fn parse_table(xml: &str) -> Result<Table, String> {
    let mut reader = Reader::from_str(xml);
    let mut table = Table::default();
    let mut ids: HashMap<String, Cell> = HashMap::new();
    let mut stack: Vec<Open> = Vec::new();
    let mut row: Option<(usize, Vec<Cell>)> = None;
    let mut mnemonic = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.name();
                if name.as_ref() == "mnemonic" {
                    mnemonic = true;
                }
                if name.as_ref() == "row" {
                    row = Some((stack.len() + 1, Vec::new()));
                }
                let (id, reference, fmt) = attributes(&element);
                let cell = row
                    .as_ref()
                    .is_some_and(|(depth, _)| stack.len() == *depth && name.as_ref() != "row");
                if let (true, Some(reference)) = (cell, reference) {
                    let known = ids.get(&reference).cloned().unwrap_or_default();
                    stack.push(Open {
                        id,
                        fmt: known.fmt,
                        text: known.text.unwrap_or_default(),
                        cell,
                    });
                } else {
                    stack.push(Open {
                        id,
                        fmt,
                        text: String::new(),
                        cell,
                    });
                }
            }
            Ok(Event::Empty(element)) => {
                let (id, reference, fmt) = attributes(&element);
                let value = match reference {
                    Some(reference) => ids.get(&reference).cloned().unwrap_or_default(),
                    None => Cell { text: None, fmt },
                };
                if let Some(id) = id {
                    ids.insert(id, value.clone());
                }
                if let Some((depth, cells)) = row.as_mut()
                    && stack.len() == *depth
                {
                    cells.push(value);
                }
            }
            Ok(Event::Text(text)) => {
                let text = text.xml_content(XmlVersion::Implicit1_0);
                if mnemonic {
                    table.columns.push(text.trim().to_owned());
                }
                if let Some(open) = stack.last_mut() {
                    open.text.push_str(&text);
                }
            }
            Ok(Event::End(element)) => {
                if element.name().as_ref() == "mnemonic" {
                    mnemonic = false;
                }
                let Some(open) = stack.pop() else {
                    continue;
                };
                let value = Cell {
                    text: (!open.text.trim().is_empty()).then(|| open.text.trim().to_owned()),
                    fmt: open.fmt,
                };
                if let Some(id) = open.id {
                    ids.insert(id, value.clone());
                }
                if open.cell
                    && let Some((_, cells)) = row.as_mut()
                {
                    cells.push(value);
                }
                if element.name().as_ref() == "row"
                    && let Some((_, cells)) = row.take()
                {
                    table.rows.push(cells);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("a trace's table: {error}")),
        }
    }
    Ok(table)
}

/// The table of the schema `schema` in the trace at `trace`, of its first
/// run.
pub fn table(trace: &Path, schema: &str) -> Result<Table, String> {
    let xpath = format!("/trace-toc/run[@number=\"1\"]/data/table[@schema=\"{schema}\"]");
    let output = Command::new("/usr/bin/xcrun")
        .args(["xctrace", "export", "--input"])
        .arg(trace)
        .args(["--xpath", &xpath])
        .output()
        .map_err(|error| format!("xctrace export: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "xctrace export of {schema} failed: {}",
            output.status
        ));
    }
    parse_table(&String::from_utf8_lossy(&output.stdout))
}

/// When the trace at `trace` began, by the wall clock, as its table of
/// contents tells it, to the millisecond.
pub fn start(trace: &Path) -> Result<SystemTime, String> {
    let output = Command::new("/usr/bin/xcrun")
        .args(["xctrace", "export", "--input"])
        .arg(trace)
        .arg("--toc")
        .output()
        .map_err(|error| format!("xctrace export: {error}"))?;
    let toc = String::from_utf8_lossy(&output.stdout);
    let date = toc
        .split("<start-date>")
        .nth(1)
        .and_then(|rest| rest.split("</start-date>").next())
        .ok_or("the trace tells no start")?;
    parse_date(date).ok_or_else(|| format!("the trace's start `{date}` is no date"))
}

/// The time of an ISO 8601 date of milliseconds and an offset, as
/// `2026-11-24T10:27:03.893+01:00`.
fn parse_date(date: &str) -> Option<SystemTime> {
    let (day, rest) = date.split_once('T')?;
    let mut parts = day.split('-');
    let (year, month, mday): (i64, i64, i64) = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    let split = rest.rfind(['+', '-']).filter(|&at| at > 0);
    let (clock, offset) = match (rest.strip_suffix('Z'), split) {
        (Some(clock), _) => (clock, 0),
        (None, Some(at)) => {
            let (clock, offset) = rest.split_at(at);
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let (hours, minutes) = offset[1..].split_once(':')?;
            (
                clock,
                sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60),
            )
        }
        _ => (rest, 0),
    };
    let mut fields = clock.split(':');
    let (hour, minute): (i64, i64) = (fields.next()?.parse().ok()?, fields.next()?.parse().ok()?);
    let seconds: f64 = fields.next()?.parse().ok()?;
    // Days from the civil date, Howard Hinnant's algorithm.
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + mday - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let whole = days * 86_400 + hour * 3600 + minute * 60 - offset;
    let nanos = i128::from(whole) * 1_000_000_000 + (seconds * 1e9).round() as i128;
    u64::try_from(nanos)
        .ok()
        .map(|nanos| UNIX_EPOCH + Duration::from_nanos(nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORT: &str = r#"<?xml version="1.0"?>
<trace-query-result>
<node xpath='//trace-toc[1]/run[1]/data[1]/table[83]'><schema name="metal-gpu-intervals"><col><mnemonic>start</mnemonic><name>Creation</name></col><col><mnemonic>duration</mnemonic><name>Duration</name></col><col><mnemonic>frame-number</mnemonic><name>Frame</name></col><col><mnemonic>process</mnemonic><name>Process</name></col></schema>
<row><start-time id="1" fmt="00:00.498.273">498273166</start-time><duration id="2" fmt="20.17 µs">20167</duration><gpu-frame-number id="3" fmt="Frame 1">1</gpu-frame-number><process id="4" fmt="ghostty (78543)"><pid id="5" fmt="78543">78543</pid></process></row>
<row><start-time id="6" fmt="00:00.498.293">498293458</start-time><duration ref="2"/><gpu-frame-number ref="3"/><process ref="4"/></row>
<row><start-time id="7" fmt="00:00.499.447">499447500</start-time><duration id="8" fmt="21.21 µs">21208</duration><sentinel/><process id="9" fmt="Code Helper (894)"><pid id="10" fmt="894">894</pid></process></row>
</node></trace-query-result>"#;

    #[test]
    fn a_tables_references_resolve_to_their_values() {
        let table = parse_table(EXPORT).unwrap();
        assert_eq!(
            table.columns,
            ["start", "duration", "frame-number", "process"]
        );
        assert_eq!(table.rows.len(), 3);
        let duration = table.column("duration").unwrap();
        let process = table.column("process").unwrap();
        let frame = table.column("frame-number").unwrap();
        assert_eq!(table.rows[1][duration].number(), Some(20167));
        assert_eq!(table.rows[1][process].pid(), Some(78543));
        assert_eq!(table.rows[1][frame].number(), Some(1));
        assert_eq!(table.rows[2][frame], Cell::default());
        assert_eq!(table.rows[2][process].pid(), Some(894));
        assert_eq!(table.rows[0][0].number(), Some(498_273_166));
    }

    #[test]
    fn a_traces_date_reads_to_the_millisecond() {
        let date = parse_date("2026-11-24T10:27:03.893+01:00").unwrap();
        let since = date.duration_since(UNIX_EPOCH).unwrap();
        // 2026-11-24T09:27:03.893Z.
        assert_eq!(since.as_millis(), 1_795_512_423_893);
        assert_eq!(parse_date("2026-11-24T09:27:03.893Z"), Some(date));
        assert_eq!(parse_date("yesterday"), None);
    }

    #[test]
    fn the_temporary_folders_volume_has_free_bytes() {
        assert!(free_bytes(&std::env::temp_dir()).unwrap() > 0);
    }
}
