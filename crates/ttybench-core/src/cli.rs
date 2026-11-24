//! The arguments that the tools share: `--key value` options and flags,
//! and the terminal that a tool measures, of its file of terminals.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::FromStr;

use crate::config::{self, Terminal, Terminals};

/// A tool's arguments, as `--rounds 10 --json`.
#[derive(Clone, Debug, Default)]
pub struct Args {
    values: BTreeMap<String, String>,
    flags: BTreeSet<String>,
}

impl Args {
    /// The arguments of `args`, of the options `options`, which take a
    /// value each, and the flags `flags`, which take none; why not
    /// otherwise.
    pub fn parse(
        args: impl IntoIterator<Item = String>,
        options: &[&str],
        flags: &[&str],
    ) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let Some(name) = arg.strip_prefix("--") else {
                return Err(format!("`{arg}` is no option"));
            };
            if flags.contains(&name) {
                parsed.flags.insert(name.to_owned());
            } else if options.contains(&name) {
                let value = args
                    .next()
                    .ok_or_else(|| format!("--{name} takes a value"))?;
                parsed.values.insert(name.to_owned(), value);
            } else {
                return Err(format!("--{name} is no option of this tool"));
            }
        }
        Ok(parsed)
    }

    /// The value of the option `name`, if given.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Whether the flag `name` is given.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    /// The value of the option `name` read as a `T`, or `default` of none.
    pub fn number<T: FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        self.value(name).map_or(Ok(default), |value| {
            value
                .parse()
                .map_err(|_| format!("--{name} takes a number, not `{value}`"))
        })
    }
}

/// The file of terminals that a tool reads: `--terminals`, else the
/// variable `TTYBENCH_TERMINALS`, else `terminals.toml` in the folder it
/// runs in, else `~/.config/ttybench/terminals.toml`.
pub fn terminals_file(args: &Args) -> PathBuf {
    if let Some(path) = args.value("terminals") {
        return PathBuf::from(config::expand_home(path));
    }
    if let Some(path) = std::env::var_os("TTYBENCH_TERMINALS") {
        return PathBuf::from(path);
    }
    let here = PathBuf::from("terminals.toml");
    if here.exists() {
        return here;
    }
    PathBuf::from(config::expand_home("~/.config/ttybench/terminals.toml"))
}

/// The exit status of a tool that measured nothing as the terminal has
/// not the feature that `--feature` names, which a session tells skipped.
pub const SKIPPED: u8 = 3;

/// The feature that `--feature` names, if one.
pub fn feature(args: &Args) -> Option<&str> {
    args.value("feature")
}

/// The terminal that `--terminal` names, of its file, of the feature that
/// `--feature` names on, if one; why none otherwise. A terminal that has
/// not the feature measures nothing alike: the tool tells it skipped and
/// ends of [`SKIPPED`].
pub fn terminal(args: &Args) -> Result<(String, Terminal), String> {
    let (name, terminal) = base_terminal(args)?;
    let Some(feature) = feature(args) else {
        return Ok((name, terminal));
    };
    match terminal.with_feature(feature)? {
        Some(terminal) => Ok((name, terminal)),
        None => {
            eprintln!(
                "{name} has no feature `{feature}` in the terminals' file: skipped, as a measure \
                 of it would measure another thing"
            );
            std::process::exit(i32::from(SKIPPED));
        }
    }
}

/// The terminal that `--terminal` names, of its file, as its base.
fn base_terminal(args: &Args) -> Result<(String, Terminal), String> {
    let path = terminals_file(args);
    let terminals: Terminals = config::read(&path).map_err(|error| error.to_string())?;
    let name = args
        .value("terminal")
        .ok_or("--terminal names the terminal to measure")?;
    let terminal = terminals.terminals.get(name).cloned().ok_or_else(|| {
        let known: Vec<&str> = terminals.terminals.keys().map(String::as_str).collect();
        format!(
            "{} describes no terminal `{name}`, of {}",
            path.display(),
            known.join(", ")
        )
    })?;
    Ok((name.to_owned(), terminal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_flags_and_numbers() {
        let args = Args::parse(
            ["--rounds", "5", "--json", "--pause", "1.5"].map(String::from),
            &["rounds", "pause"],
            &["json"],
        )
        .unwrap();
        assert_eq!(args.number("rounds", 10_u32), Ok(5));
        assert_eq!(args.number("pause", 2.0_f64), Ok(1.5));
        assert_eq!(args.number("season", 0_u32), Ok(0));
        assert!(args.flag("json") && !args.flag("other"));
        for bad in [vec!["--rounds"], vec!["--nope", "1"], vec!["rounds"]] {
            assert!(Args::parse(bad.into_iter().map(String::from), &["rounds"], &[]).is_err());
        }
        let args = Args::parse(["--rounds", "x"].map(String::from), &["rounds"], &[]).unwrap();
        assert!(args.number("rounds", 1_u32).is_err());
    }

    #[test]
    fn a_terminal_of_its_file() {
        let file = std::env::temp_dir().join(format!("ttybench-cli-{}.toml", std::process::id()));
        std::fs::write(
            &file,
            "[terminals.sh]\napp = \"/bin\"\nlaunch = [\"/bin/sh\", \"{command}\"]\n",
        )
        .unwrap();
        let path = file.to_string_lossy().into_owned();
        let args = Args::parse(
            [
                "--terminals".to_owned(),
                path.clone(),
                "--terminal".to_owned(),
                "sh".to_owned(),
            ],
            &["terminals", "terminal"],
            &[],
        )
        .unwrap();
        assert_eq!(terminal(&args).unwrap().0, "sh");
        let args = Args::parse(
            [
                "--terminals".to_owned(),
                path,
                "--terminal".to_owned(),
                "zsh".to_owned(),
            ],
            &["terminals", "terminal"],
            &[],
        )
        .unwrap();
        assert!(terminal(&args).unwrap_err().contains("of sh"));
        let _ = std::fs::remove_file(&file);
    }
}
