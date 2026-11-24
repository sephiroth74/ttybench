//! A session of two terminals and two tools, of a probe of a script's in
//! place of the tools, which opens no window: its orders, its records,
//! a run that fails, a run skipped, the session taken up, and its report.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A tool that writes a record of its terminal and its rounds into its
/// session, or fails for the terminal that `TTYBENCH_PROBE_FAILS` names,
/// or skips the one that `TTYBENCH_PROBE_SKIPS` names, as of a feature
/// that it has not.
const PROBE: &str = r#"#!/bin/sh
while [ $# -gt 0 ]; do
  case "$1" in
    --session) session="$2"; shift 2;;
    --terminal) terminal="$2"; shift 2;;
    --rounds) rounds="$2"; shift 2;;
    *) shift;;
  esac
done
if [ "$terminal" = "$TTYBENCH_PROBE_FAILS" ]; then
  echo "no luck for $terminal" >&2
  exit 4
fi
if [ "$terminal" = "$TTYBENCH_PROBE_SKIPS" ]; then
  echo "$terminal has no feature: skipped" >&2
  exit 3
fi
mkdir -p "$session"
printf '{"tool":"%s","terminal":"%s","metric":"rounds","unit":"n","samples":[%s]}\n' \
  "$(basename "$0")" "$terminal" "${rounds:-0}" >> "$session/records.jsonl"
echo "probed $terminal"
"#;

/// A report of a script's, where the real one is not built beside the
/// coordinator.
const REPORT: &str = "#!/bin/sh\necho reported > \"$1/report.md\"\n";

const TERMINALS: &str = r#"
[terminals.a]
app = "/bin"
launch = ["/bin/sh", "{command}"]

[terminals.b]
app = "/bin"
launch = ["/bin/sh", "{command}"]
"#;

const SESSION: &str = r#"
terminals = ["a", "b"]
terminals-file = "terminals.toml"
folder = "out"
pause = 0
cycles = 2
rounds = 3

[[run]]
tool = "probe"

[[run]]
tool = "probe-again"
rounds = 7
"#;

/// A folder of the probe's two tools, the terminals and the session.
fn place(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ttybench-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    for tool in ["probe", "probe-again"] {
        let path = dir.join("bin").join(format!("ttybench-{tool}"));
        std::fs::write(&path, PROBE).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let report = dir.join("bin").join("ttybench-report");
    std::fs::write(&report, REPORT).unwrap();
    std::fs::set_permissions(&report, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("terminals.toml"), TERMINALS).unwrap();
    std::fs::write(dir.join("session.toml"), SESSION).unwrap();
    dir
}

fn coordinator(dir: &Path, args: &[&str], fails: Option<&str>) -> Output {
    coordinator_with(dir, args, fails, None)
}

fn coordinator_with(dir: &Path, args: &[&str], fails: Option<&str>, skips: Option<&str>) -> Output {
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_ttybench-run"));
    command.args(args).current_dir(dir).env("PATH", path);
    match fails {
        Some(terminal) => command.env("TTYBENCH_PROBE_FAILS", terminal),
        None => command.env_remove("TTYBENCH_PROBE_FAILS"),
    };
    match skips {
        Some(terminal) => command.env("TTYBENCH_PROBE_SKIPS", terminal),
        None => command.env_remove("TTYBENCH_PROBE_SKIPS"),
    };
    command.output().unwrap()
}

fn lines(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_session_in_balanced_orders_cut_off_and_taken_up() {
    let dir = place("session");
    let out = dir.join("out");

    let first = coordinator(&dir, &["session.toml"], Some("b"));
    assert_eq!(
        first.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let log = lines(&out.join("runs.jsonl"));
    let order: Vec<(u64, u64, &str, &str)> = log
        .iter()
        .map(|entry| {
            (
                entry["cycle"].as_u64().unwrap(),
                entry["run"].as_u64().unwrap(),
                entry["terminal"].as_str().unwrap(),
                entry["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            (1, 1, "a", "done"),
            (1, 1, "b", "failed"),
            (1, 2, "a", "done"),
            (1, 2, "b", "failed"),
            (2, 1, "b", "failed"),
            (2, 1, "a", "done"),
            (2, 2, "b", "failed"),
            (2, 2, "a", "done"),
        ]
    );
    assert!(
        log[1]["error"]
            .as_str()
            .unwrap()
            .contains("exited of 4:\nno luck for b")
    );
    let records = lines(&out.join("records.jsonl"));
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|record| record["terminal"] == "a"));
    assert_eq!(records[0]["samples"][0].as_f64(), Some(3.0));
    assert_eq!(records[1]["samples"][0].as_f64(), Some(7.0));
    assert_eq!(records[1]["tool"], "ttybench-probe-again");
    assert_eq!(records[3]["cycle"], 2);
    for kept in [
        "session.toml",
        "terminals.toml",
        "machine.json",
        "runs/c1-r1-probe-a/output.txt",
        "report.md",
    ] {
        assert!(out.join(kept).exists(), "{kept}");
    }

    let again = coordinator(&dir, &["session.toml"], None);
    assert_eq!(again.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&again.stderr).contains("--resume"));

    let resumed = coordinator(&dir, &["--resume", "out"], None);
    assert_eq!(
        resumed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&resumed.stderr).contains("4 of 8 runs ended well before, 4 to go")
    );
    assert_eq!(lines(&out.join("runs.jsonl")).len(), 12);
    let records = lines(&out.join("records.jsonl"));
    assert_eq!(records.len(), 8);
    let terminals: Vec<&str> = records
        .iter()
        .map(|record| record["terminal"].as_str().unwrap())
        .collect();
    assert_eq!(terminals, ["a", "a", "a", "a", "b", "b", "b", "b"]);

    let ended = coordinator(&dir, &["--resume", "out"], None);
    assert_eq!(ended.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&ended.stdout).contains("every run"));
    assert_eq!(lines(&out.join("records.jsonl")).len(), 8);
    assert!(out.join("report.md").exists());

    std::fs::remove_file(out.join("report.md")).unwrap();
    let quiet = coordinator(&dir, &["--resume", "out", "--no-report"], None);
    assert_eq!(quiet.status.code(), Some(0));
    assert!(!out.join("report.md").exists());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_dry_run_tells_the_steps_and_writes_nothing() {
    let dir = place("dry");
    let output = coordinator(&dir, &["session.toml", "--dry-run"], None);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        text.lines().filter(|line| line.contains("cycle")).count(),
        8
    );
    assert!(text.contains("cycle 2, probe of b, 1 of its order: ttybench probe --rounds 3"));
    assert!(!dir.join("out").exists());
    let unknown = coordinator(&dir, &["--resume", "nowhere"], None);
    assert_eq!(unknown.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_run_of_a_feature_that_its_terminal_has_not_is_skipped_and_stays_so() {
    let dir = place("skipped");
    let out = dir.join("out");
    let first = coordinator_with(&dir, &["session.toml"], None, Some("b"));
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let log = lines(&out.join("runs.jsonl"));
    let skipped: Vec<&str> = log
        .iter()
        .filter(|entry| entry["status"] == "skipped")
        .map(|entry| entry["terminal"].as_str().unwrap())
        .collect();
    assert_eq!(skipped, ["b"; 4]);
    assert!(
        log.iter()
            .filter(|entry| entry["status"] == "skipped")
            .all(|entry| entry["error"] == "b has no feature: skipped")
    );
    assert!(String::from_utf8_lossy(&first.stdout).contains("0 failed, 4 skipped"));
    assert_eq!(lines(&out.join("records.jsonl")).len(), 4);
    // Taken up, it has nothing more to run.
    let again = coordinator_with(&dir, &["--resume", "out"], None, None);
    assert_eq!(again.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&again.stdout).contains("every run"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_terminal_of_no_feature_of_a_run_is_skipped_of_no_launch() {
    let dir = place("feature");
    std::fs::write(
        dir.join("terminals.toml"),
        format!("{TERMINALS}features.smooth = {{ env = {{ SMOOTH = \"1\" }} }}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("session.toml"),
        "terminals = [\"a\", \"b\"]\nterminals-file = \"terminals.toml\"\nfolder = \"out\"\npause = 0\n\n[[run]]\ntool = \"probe\"\nfeature = \"smooth\"\n",
    )
    .unwrap();
    let dry = coordinator(&dir, &["session.toml", "--dry-run"], None);
    let listed = String::from_utf8_lossy(&dry.stdout).into_owned();
    assert!(
        listed.contains("probe of b, 2 of its order: ttybench probe --feature smooth\n"),
        "{listed}"
    );
    assert!(
        listed.contains("probe of a, 1 of its order: ttybench probe --feature smooth (skipped: no feature `smooth`)"),
        "{listed}"
    );
    let run = coordinator(&dir, &["session.toml"], None);
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let log = lines(&dir.join("out/runs.jsonl"));
    let statuses: Vec<(&str, &str)> = log
        .iter()
        .map(|entry| {
            (
                entry["terminal"].as_str().unwrap(),
                entry["status"].as_str().unwrap(),
            )
        })
        .collect();
    // The terminal b, of the terminals' file's last, has the feature: a not.
    assert_eq!(statuses, [("a", "skipped"), ("b", "done")]);
    assert!(!dir.join("out/runs/c1-r1-probe-a").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
