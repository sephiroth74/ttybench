//! A terminal launched for a measure: of its config's launch, a program's
//! arguments or `open`, running the command that a tool gives; its tree of
//! processes, its window, the keys typed into it, and its quit, every
//! process of its tree gone.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{Key, Launch, Quit, Terminal};
use crate::{keys, process, windows};

/// How long a launch by `open` waits for the app's new process.
pub const OPEN_WAIT: Duration = Duration::from_secs(15);

/// How often a wait looks again.
const POLL: Duration = Duration::from_millis(10);

/// A terminal that a tool launched.
#[derive(Debug)]
pub struct Launched {
    /// Its name, as its config calls it.
    pub name: String,
    /// Its first process: the program launched, or the app's new process
    /// of a launch by `open`.
    pub root: libc::pid_t,
    /// When its launch started.
    pub started: Instant,
    /// The program launched, which its quit reaps; none of `open`'s.
    child: Option<Child>,
    /// How it quits.
    quit: Quit,
}

/// The processes of the app `app` that run, its own executables', as a
/// copy that a run would meet.
pub fn running_copies(app: &Path) -> Vec<libc::pid_t> {
    let executables = app.join("Contents/MacOS");
    process::all_pids()
        .unwrap_or_default()
        .into_iter()
        .filter(|&pid| process::path(pid).is_ok_and(|path| path.starts_with(&executables)))
        .collect()
}

/// A script in `dir` named `name`, of `body` after its interpreter's line,
/// which its owner may run.
pub fn script(dir: &Path, name: &str, body: &str) -> io::Result<PathBuf> {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n"))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

/// Launches the terminal `name` of `terminal` running `command`, a script
/// that its owner may run, in `cwd`, of `env` beside its config's
/// environment. A launch by `open` opens a `.command` script beside
/// `command` that runs it, and takes the app's new process, which it waits
/// for [`OPEN_WAIT`].
pub fn launch(
    name: &str,
    terminal: &Terminal,
    command: &Path,
    cwd: &Path,
    env: &[(String, String)],
) -> io::Result<Launched> {
    let started = Instant::now();
    let variables: Vec<(String, String)> = terminal
        .env
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .chain(env.iter().cloned())
        .collect();
    match &terminal.launch {
        Launch::Argv(_) => {
            let argv = terminal.argv(command, cwd).expect("a launch of arguments");
            let child = Command::new(&argv[0])
                .args(&argv[1..])
                .envs(variables)
                .current_dir(cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            let root = libc::pid_t::try_from(child.id()).map_err(io::Error::other)?;
            Ok(Launched {
                name: name.to_owned(),
                root,
                started,
                child: Some(child),
                quit: terminal.quit,
            })
        }
        Launch::Open => {
            let dir = command.parent().unwrap_or(cwd);
            let stem = command
                .file_stem()
                .map_or_else(|| "ttybench".into(), |stem| stem.to_string_lossy());
            let quoted = command.to_string_lossy().replace('\'', "'\\''");
            let opened = script(
                dir,
                &format!("{stem}.command"),
                &format!(
                    "cd '{}' && exec '{quoted}'",
                    cwd.to_string_lossy().replace('\'', "'\\''")
                ),
            )?;
            let before = running_copies(&terminal.app);
            let mut open = Command::new("/usr/bin/open");
            open.arg("-n").arg("-a").arg(&terminal.app);
            for (name, value) in &variables {
                open.arg("--env").arg(format!("{name}={value}"));
            }
            let status = open
                .arg(&opened)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                return Err(io::Error::other(format!(
                    "`open` of {} failed: {status}",
                    terminal.app.display()
                )));
            }
            let deadline = Instant::now() + OPEN_WAIT;
            loop {
                if let Some(&root) = running_copies(&terminal.app)
                    .iter()
                    .find(|pid| !before.contains(pid))
                {
                    return Ok(Launched {
                        name: name.to_owned(),
                        root,
                        started,
                        child: None,
                        quit: terminal.quit,
                    });
                }
                if Instant::now() > deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("no new process of {} came", terminal.app.display()),
                    ));
                }
                thread::sleep(POLL);
            }
        }
    }
}

/// Launches the terminal `name` of `terminal` running `payload`, in
/// `cwd`, and waits for the payload's `ready` within `timeout`: the
/// terminal and the event; the terminal quit of one that does not come.
pub fn launch_ready(
    name: &str,
    terminal: &Terminal,
    payload: &crate::payload::Payload,
    cwd: &Path,
    timeout: Duration,
) -> Result<(Launched, serde_json::Value), String> {
    let launched = launch(name, terminal, payload.command(), cwd, &[])
        .map_err(|error| format!("{name} does not launch: {error}"))?;
    match payload.wait("ready", timeout) {
        Some(ready) => Ok((launched, ready)),
        None => {
            let _ = launched.quit(Duration::from_secs(5));
            Err(format!(
                "{name} ran no payload within {} s",
                timeout.as_secs()
            ))
        }
    }
}

impl Launched {
    /// Its tree of processes, its first first.
    pub fn tree(&self) -> Vec<libc::pid_t> {
        process::tree(self.root)
    }

    /// Its largest window on screen, of any process of its tree.
    pub fn window(&self) -> Option<windows::Window> {
        windows::window_of(&self.tree())
    }

    /// Its window, once it shows, within `timeout`.
    pub fn wait_window(&self, timeout: Duration) -> Option<windows::Window> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(window) = self.window() {
                return Some(window);
            }
            if Instant::now() > deadline {
                return None;
            }
            thread::sleep(POLL);
        }
    }

    /// Presses `key` in the process of its window, or its first; whether
    /// the key has a code to press.
    pub fn press(&self, key: &Key) -> bool {
        let pid = self.window().map_or(self.root, |window| window.pid);
        keys::press(pid, key)
    }

    /// Quits it, as its config says, every process of its tree gone within
    /// `deadline` or killed past it.
    pub fn quit(mut self, deadline: Duration) -> io::Result<()> {
        let tree = self.tree();
        match self.quit {
            Quit::App => terminate_app(self.root),
            Quit::Signal => {
                for &pid in &tree {
                    let _ = process::terminate(pid);
                }
            }
        }
        let until = Instant::now() + deadline;
        while tree.iter().any(|&pid| process::exists(pid)) && Instant::now() < until {
            self.reap();
            thread::sleep(POLL);
        }
        for &pid in &tree {
            if process::exists(pid) {
                let _ = process::kill(pid);
            }
        }
        if let Some(child) = self.child.as_mut() {
            child.wait()?;
        }
        Ok(())
    }

    /// Reaps its program launched, if it ended, so that its process goes.
    fn reap(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.try_wait();
        }
    }
}

/// Asks the app of the process `pid` to quit, as its menu's Quit, of its
/// running application's `terminate`.
fn terminate_app(pid: libc::pid_t) {
    use objc2_app_kit::NSRunningApplication;
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, Terminals};

    /// A folder of the tests' own, empty, named after `name`.
    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ttybench-terminal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_launch_of_arguments_runs_its_command_and_quits_its_tree() {
        // A "terminal" of `/bin/sh`, whose command starts a child.
        let dir = folder("argv");
        let terminals: Terminals = config::parse(
            Path::new("t.toml"),
            "[terminals.sh]\napp = \"/bin\"\nlaunch = [\"/bin/sh\", \"{command}\"]\n",
        )
        .unwrap();
        let marker = dir.join("ran");
        let command = script(
            &dir,
            "run.sh",
            &format!("touch '{}'; /bin/sleep 30 & wait", marker.display()),
        )
        .unwrap();
        let launched = launch("sh", &terminals.terminals["sh"], &command, &dir, &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        assert!(marker.exists());
        let tree = launched.tree();
        assert!(tree.len() >= 2, "{tree:?}");
        launched.quit(Duration::from_secs(2)).unwrap();
        assert!(tree.iter().all(|&pid| !process::exists(pid)), "{tree:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Launches `name` of the examples, finds its window and quits it.
    fn launch_example(name: &str) {
        let terminals: Terminals = config::read(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/terminals.toml"
        )))
        .unwrap();
        let terminal = &terminals.terminals[name];
        if !terminal.app.exists() {
            eprintln!("{name} is not installed: skipped");
            return;
        }
        let dir = folder(name);
        let command = script(&dir, "run", "exec /bin/sleep 30").unwrap();
        let launched = launch(name, terminal, &command, &dir, &[]).unwrap();
        let window = launched.wait_window(Duration::from_secs(15));
        let tree = launched.tree();
        launched.quit(Duration::from_secs(5)).unwrap();
        assert!(window.is_some(), "{name} showed no window");
        assert!(tree.iter().all(|&pid| !process::exists(pid)), "{tree:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "opens windows: run it by hand, `cargo test -- --ignored`"]
    fn terminal_app_launches_by_open_shows_its_window_and_quits() {
        launch_example("terminal");
    }

    #[test]
    #[ignore = "opens windows: run it by hand, `cargo test -- --ignored`"]
    fn a_terminal_of_arguments_launches_shows_its_window_and_quits() {
        launch_example("kitty");
        launch_example("ghostty");
    }
}
