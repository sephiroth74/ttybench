//! ttybench's tools, each a binary of its own named `ttybench-<tool>`,
//! which the dispatcher finds beside its own binary, then on the `PATH`:
//! a tool of someone else's, of that name, joins them.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The prefix of a tool's binary, before its name.
pub const PREFIX: &str = "ttybench-";

/// A tool that the dispatcher found: its name, as `latency` of
/// `ttybench-latency`, and its binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    /// Its name, which `ttybench <name>` runs.
    pub name: String,
    /// Its binary.
    pub path: PathBuf,
}

/// Whether `name` can name a tool: of ASCII letters, digits and `-`, of a
/// letter first, so that no path nor option reads in it.
pub fn is_tool_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Where the dispatcher looks for its tools, in their order: the folder of
/// its own binary, `own`, then those of `path`, the `PATH`'s value, each
/// once.
pub fn search_dirs(own: Option<&Path>, path: Option<&OsStr>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = own
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .into_iter()
        .collect();
    if let Some(path) = path {
        for dir in std::env::split_paths(path) {
            if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

/// The binary of the tool `name`: the first of `dirs` that holds a file
/// `ttybench-<name>` that its owner may run.
pub fn find(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if !is_tool_name(name) {
        return None;
    }
    dirs.iter()
        .map(|dir| dir.join(format!("{PREFIX}{name}")))
        .find(|path| is_executable(path))
}

/// The tools that `dirs` hold, each name once, of the first folder that
/// holds it, in their names' order.
pub fn list(dirs: &[PathBuf]) -> Vec<Tool> {
    let mut seen = BTreeSet::new();
    let mut tools = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path
                .file_name()
                .and_then(OsStr::to_str)
                .and_then(|file| file.strip_prefix(PREFIX))
            else {
                continue;
            };
            if is_tool_name(name) && is_executable(&path) && seen.insert(name.to_owned()) {
                tools.push(Tool {
                    name: name.to_owned(),
                    path,
                });
            }
        }
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

/// Whether `path` is a file that its owner may run.
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o100 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the tests' own, empty, named after `name`.
    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ttybench-tools-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file at `path`, which its owner may run or not.
    fn file(path: &Path, runs: bool) {
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        let mode = if runs { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn a_tool_s_name_is_of_letters_digits_and_dashes_of_a_letter_first() {
        for name in ["latency", "gpu", "frames-2"] {
            assert!(is_tool_name(name), "{name}");
        }
        for name in ["", "-v", "../x", "a/b", "2x", "a b", "é"] {
            assert!(!is_tool_name(name), "{name}");
        }
    }

    #[test]
    fn the_tools_are_found_beside_the_binary_then_on_the_path_each_once() {
        let own = folder("own");
        let other = folder("other");
        file(&own.join("ttybench-latency"), true);
        file(&own.join("ttybench-notes"), false);
        file(&own.join("ttybench"), true);
        file(&other.join("ttybench-latency"), true);
        file(&other.join("ttybench-gpu"), true);
        file(&other.join("ttybench-..bad"), true);
        let path = std::env::join_paths([&other, &own]).unwrap();
        let dirs = search_dirs(Some(&own.join("ttybench")), Some(&path));
        // Its own folder first, the path's each once.
        assert_eq!(dirs, [own.clone(), other.clone()]);
        let tools = list(&dirs);
        let names: Vec<(&str, &Path)> = tools
            .iter()
            .map(|tool| (tool.name.as_str(), tool.path.as_path()))
            .collect();
        assert_eq!(
            names,
            [
                ("gpu", other.join("ttybench-gpu").as_path()),
                ("latency", own.join("ttybench-latency").as_path()),
            ]
        );
        assert_eq!(find("latency", &dirs), Some(own.join("ttybench-latency")));
        assert_eq!(find("notes", &dirs), None);
        assert_eq!(find("../latency", &dirs), None);
        let _ = std::fs::remove_dir_all(&own);
        let _ = std::fs::remove_dir_all(&other);
    }
}
