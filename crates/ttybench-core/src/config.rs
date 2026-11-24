//! The terminals that a user measures, as their file describes them,
//! `terminals.toml`: how each launches with a command, its environment, the
//! keys that set a scenario up, and how it quits. ttybench knows no
//! terminal by name: everything that tells one from another is here.
//!
//! ```toml
//! [terminals.ghostty]
//! app = "/Applications/Ghostty.app"
//! launch = ["{app}/Contents/MacOS/ghostty", "-e", "{command}"]
//! env = { XDG_CONFIG_HOME = "~/ttybench/config" }
//!
//! [terminals.iterm2]
//! app = "/Applications/iTerm.app"
//! launch = "open"
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer};

/// The placeholders of a launch's arguments.
pub const PLACEHOLDERS: [&str; 3] = ["{app}", "{command}", "{cwd}"];

/// The terminals of a file, by their names.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminals {
    /// Each terminal, by the name that the tools and the sessions call it.
    #[serde(default)]
    pub terminals: BTreeMap<String, Terminal>,
}

/// A terminal to measure.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    /// Its app's bundle, as `/Applications/Ghostty.app`, a path absolute or
    /// from `~`.
    #[serde(deserialize_with = "path")]
    pub app: PathBuf,
    /// How it launches with a command.
    pub launch: Launch,
    /// The variables that its environment gets beside the tool's, each
    /// value of `~` expanded, as the folder of its config of the session.
    #[serde(default, deserialize_with = "environment")]
    pub env: BTreeMap<String, String>,
    /// The keys that set a scenario up once it is ready, in their order,
    /// as `["cmd+d", "alt+cmd+i"]`.
    #[serde(default)]
    pub setup: Vec<Key>,
    /// How it quits once measured.
    #[serde(default)]
    pub quit: Quit,
}

/// How a terminal launches with a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Its program and arguments, of the placeholders `{app}`, its bundle,
    /// `{command}`, the script that it runs, and `{cwd}`, the folder it
    /// starts in.
    Argv(Vec<String>),
    /// Through `open -n -a`, of a `.command` script that the app opens, for
    /// a terminal that takes no command on its command line.
    Open,
}

/// How a terminal quits once measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quit {
    /// A `SIGTERM` to its processes, then a `SIGKILL` past a deadline.
    #[default]
    Signal,
    /// The app's own quit, as its menu's, then a signal past a deadline.
    App,
}

/// A key that a tool types into a terminal: its modifiers and its key, as
/// `cmd+shift+d` writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// Command held.
    pub command: bool,
    /// Control held.
    pub control: bool,
    /// Option held.
    pub option: bool,
    /// Shift held.
    pub shift: bool,
    /// The key: a character of the US layout, lowercase, or a name of
    /// [`NAMED_KEYS`].
    pub key: String,
}

/// The keys that a name writes, beside the characters.
pub const NAMED_KEYS: [&str; 22] = [
    "enter",
    "tab",
    "space",
    "escape",
    "backspace",
    "delete",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
];

impl Key {
    /// The key that `text` writes, as `cmd+shift+d`: modifiers, each
    /// followed by `+`, then the key; why not otherwise.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut key = Self {
            command: false,
            control: false,
            option: false,
            shift: false,
            key: String::new(),
        };
        // The last part is the key, `+` itself of `cmd++`.
        let (mods, last) = match text.strip_suffix("++") {
            Some(mods) => (mods, "+"),
            None => match text.rsplit_once('+') {
                Some((mods, last)) => (mods, last),
                None => ("", text),
            },
        };
        for modifier in mods.split('+').filter(|part| !part.is_empty()) {
            let held = match modifier.to_ascii_lowercase().as_str() {
                "cmd" | "command" => &mut key.command,
                "ctrl" | "control" => &mut key.control,
                "alt" | "opt" | "option" => &mut key.option,
                "shift" => &mut key.shift,
                other => return Err(format!("`{other}` is no modifier, in the key `{text}`")),
            };
            *held = true;
        }
        let name = last.to_ascii_lowercase();
        let single = name.chars().count() == 1 && name.is_ascii() && !name.trim().is_empty();
        if !single && !NAMED_KEYS.contains(&name.as_str()) {
            return Err(format!("`{last}` is no key, in the key `{text}`"));
        }
        key.key = name;
        Ok(key)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Key::parse(&text).map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for Launch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Mode(String),
            Argv(Vec<String>),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Mode(mode) if mode == "open" => Ok(Launch::Open),
            Raw::Mode(mode) => Err(de::Error::custom(format!(
                "a launch is a program's arguments or \"open\", not \"{mode}\""
            ))),
            Raw::Argv(argv) => check_argv(&argv)
                .map(|()| Launch::Argv(argv))
                .map_err(de::Error::custom),
        }
    }
}

/// Whether `argv` is a launch's: a program and its arguments, of the
/// placeholders alone, `{command}` among them; why not otherwise.
fn check_argv(argv: &[String]) -> Result<(), String> {
    if argv.first().is_none_or(|program| program.trim().is_empty()) {
        return Err("a launch names its program first".to_owned());
    }
    for arg in argv {
        let mut rest = arg.as_str();
        while let Some(open) = rest.find('{') {
            let close = rest[open..]
                .find('}')
                .map(|at| open + at)
                .ok_or_else(|| format!("`{arg}` opens a placeholder that it closes not"))?;
            let name = &rest[open..=close];
            if !PLACEHOLDERS.contains(&name) {
                return Err(format!(
                    "`{name}` is no placeholder, of `{{app}}`, `{{command}}` and `{{cwd}}`"
                ));
            }
            rest = &rest[close + 1..];
        }
    }
    if !argv.iter().any(|arg| arg.contains("{command}")) {
        return Err("a launch runs the `{command}` that a tool gives".to_owned());
    }
    Ok(())
}

impl Terminal {
    /// The launch's program and arguments of `command`, the script that it
    /// runs, and `cwd`, the folder it starts in; none of a launch by
    /// `open`.
    pub fn argv(&self, command: &Path, cwd: &Path) -> Option<Vec<String>> {
        let Launch::Argv(argv) = &self.launch else {
            return None;
        };
        let app = self.app.to_string_lossy();
        let command = command.to_string_lossy();
        let cwd = cwd.to_string_lossy();
        Some(
            argv.iter()
                .map(|arg| {
                    arg.replace("{app}", &app)
                        .replace("{command}", &command)
                        .replace("{cwd}", &cwd)
                })
                .collect(),
        )
    }
}

/// `text` of a `~` at its start as the home folder.
pub fn expand_home(text: &str) -> String {
    match (text.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            format!("{}{rest}", home.to_string_lossy())
        }
        _ => text.to_owned(),
    }
}

/// A path absolute or from `~`, expanded.
fn path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    let text = String::deserialize(deserializer)?;
    let expanded = PathBuf::from(expand_home(&text));
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Err(de::Error::custom(format!(
            "`{text}` is no path absolute or from `~`"
        )))
    }
}

/// An environment's variables, each value of `~` expanded.
fn environment<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    let raw = BTreeMap::<String, String>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|(name, value)| (name, expand_home(&value)))
        .collect())
}

/// A file of the config that does not read: its path, the line and the
/// column where it fails, from 1, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    /// The file.
    pub path: PathBuf,
    /// Where it fails, if the reader tells it.
    pub at: Option<(usize, usize)>,
    /// Why.
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.at {
            Some((line, column)) => write!(
                formatter,
                "{}:{line}:{column}: {}",
                self.path.display(),
                self.message
            ),
            None => write!(formatter, "{}: {}", self.path.display(), self.message),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The value of type `T` of the TOML file at `path`, of `text`, its
/// contents: what [`Terminals`] reads, and a session's file.
pub fn parse<T: for<'de> Deserialize<'de>>(path: &Path, text: &str) -> Result<T, ConfigError> {
    toml::from_str(text).map_err(|error| ConfigError {
        path: path.to_path_buf(),
        at: error.span().map(|span| line_and_column(text, span.start)),
        message: error.message().to_owned(),
    })
}

/// The value of type `T` of the TOML file at `path`.
pub fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|error| ConfigError {
        path: path.to_path_buf(),
        at: None,
        message: error.to_string(),
    })?;
    parse(path, &text)
}

/// The line and the column, from 1, of the byte `at` of `text`.
fn line_and_column(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |last| last.chars().count())
        + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminals(text: &str) -> Result<Terminals, ConfigError> {
        parse(Path::new("terminals.toml"), text)
    }

    #[test]
    fn the_examples_read() {
        let text = include_str!("../../../examples/terminals.toml");
        let read = terminals(text).unwrap();
        let names: Vec<&str> = read.terminals.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "alacritty",
                "ghostty",
                "iterm2",
                "kitty",
                "lokyr",
                "terminal",
                "warp"
            ]
        );
        assert_eq!(read.terminals["iterm2"].launch, Launch::Open);
        let ghostty = &read.terminals["ghostty"];
        assert_eq!(
            ghostty.argv(Path::new("/tmp/run.sh"), Path::new("/tmp")),
            Some(vec![
                "/Applications/Ghostty.app/Contents/MacOS/ghostty".to_owned(),
                "-e".to_owned(),
                "/tmp/run.sh".to_owned(),
            ])
        );
    }

    #[test]
    fn a_terminal_of_its_environment_its_keys_and_its_quit() {
        let read = terminals(
            "[terminals.x]\napp = \"~/Apps/X.app\"\nlaunch = [\"{app}/x\", \"--cwd={cwd}\", \"{command}\"]\nenv = { CONF = \"~/conf\" }\nsetup = [\"cmd+d\", \"alt+cmd+i\", \"cmd++\"]\nquit = \"app\"\n",
        )
        .unwrap();
        let x = &read.terminals["x"];
        let home = std::env::var("HOME").unwrap();
        assert_eq!(x.app, PathBuf::from(format!("{home}/Apps/X.app")));
        assert_eq!(x.env["CONF"], format!("{home}/conf"));
        assert_eq!(x.quit, Quit::App);
        assert_eq!(x.setup.len(), 3);
        assert!(x.setup[1].option && x.setup[1].command && x.setup[1].key == "i");
        assert!(x.setup[2].command && x.setup[2].key == "+");
        assert_eq!(
            x.argv(Path::new("/r"), Path::new("/w")).unwrap(),
            [
                format!("{home}/Apps/X.app/x"),
                "--cwd=/w".to_owned(),
                "/r".to_owned()
            ]
        );
    }

    #[test]
    fn what_is_refused_tells_its_line_and_column() {
        // A launch of no `{command}`, of its line.
        let error =
            terminals("[terminals.x]\napp = \"/X.app\"\nlaunch = [\"/x\", \"-e\"]\n").unwrap_err();
        assert_eq!(error.at.map(|(line, _)| line), Some(3), "{error}");
        assert!(error.message.contains("{command}"), "{error}");
        // An unknown placeholder, an unknown mode, a relative app, a key of
        // no modifier, an unknown field.
        for (text, says) in [
            (
                "[terminals.x]\napp = \"/X.app\"\nlaunch = [\"/x\", \"{cmd}\"]\n",
                "{cmd}",
            ),
            (
                "[terminals.x]\napp = \"/X.app\"\nlaunch = \"shell\"\n",
                "\"shell\"",
            ),
            (
                "[terminals.x]\napp = \"X.app\"\nlaunch = \"open\"\n",
                "X.app",
            ),
            (
                "[terminals.x]\napp = \"/X.app\"\nlaunch = \"open\"\nsetup = [\"hyper+d\"]\n",
                "hyper",
            ),
            (
                "[terminals.x]\napp = \"/X.app\"\nlaunch = \"open\"\ncolour = 1\n",
                "colour",
            ),
        ] {
            let error = terminals(text).unwrap_err();
            assert!(error.to_string().contains(says), "{error}");
            assert!(error.at.is_some(), "{error}");
        }
    }

    #[test]
    fn a_key_is_of_its_modifiers_and_one_key() {
        let key = Key::parse("ctrl+shift+Tab").unwrap();
        assert!(key.control && key.shift && !key.command && key.key == "tab");
        assert_eq!(Key::parse("x").unwrap().key, "x");
        for bad in ["cmd+", "cmd+abc", "super+x", ""] {
            assert!(Key::parse(bad).is_err(), "{bad}");
        }
    }
}
