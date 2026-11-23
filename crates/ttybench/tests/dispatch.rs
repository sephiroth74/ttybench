//! The dispatcher runs a tool of the `PATH` of its arguments, its exit
//! status its own, and lists the tools that it finds.

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// A folder of a tool `ttybench-echo`, which prints its arguments and
/// exits of 3.
fn tools() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ttybench-dispatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tool = dir.join("ttybench-echo");
    std::fs::write(&tool, "#!/bin/sh\necho \"echo: $*\"\nexit 3\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

#[test]
fn a_tool_runs_of_its_arguments_and_the_help_lists_it() {
    let dir = tools();
    let path = std::env::join_paths([
        dir.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .unwrap();
    let ran = Command::new(env!("CARGO_BIN_EXE_ttybench"))
        .args(["echo", "one", "two"])
        .env("PATH", &path)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "echo: one two\n");
    assert_eq!(ran.status.code(), Some(3));
    let help = Command::new(env!("CARGO_BIN_EXE_ttybench"))
        .arg("--help")
        .env("PATH", &path)
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("  echo "), "{help}");
    // A tool not there, and a name that is no tool's.
    let missing = Command::new(env!("CARGO_BIN_EXE_ttybench"))
        .arg("nothing")
        .env("PATH", &path)
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(127));
    let bad = Command::new(env!("CARGO_BIN_EXE_ttybench"))
        .arg("../echo")
        .env("PATH", &path)
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}
