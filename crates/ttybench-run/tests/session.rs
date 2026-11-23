//! A session of two terminals and two tools, of a probe of a script's in
//! place of the tools, which opens no window: its orders, its records,
//! a run that fails, and the session taken up.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A tool that writes a record of its terminal and its rounds into its
/// session, or fails for the terminal that `TTYBENCH_PROBE_FAILS` names.
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
  exit 3
fi
mkdir -p "$session"
printf '{"tool":"%s","terminal":"%s","metric":"rounds","unit":"n","samples":[%s]}\n' \
  "$(basename "$0")" "$terminal" "${rounds:-0}" >> "$session/records.jsonl"
echo "probed $terminal"
"#;

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
    std::fs::write(dir.join("terminals.toml"), TERMINALS).unwrap();
    std::fs::write(dir.join("session.toml"), SESSION).unwrap();
    dir
}

fn coordinator(dir: &Path, args: &[&str], fails: Option<&str>) -> Output {
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
            .contains("exited of 3:\nno luck for b")
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
