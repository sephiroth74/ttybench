//! A session's file, `session.toml`: the terminals that it measures, the
//! runs of its tools, its cycles, and the steps that they make, each a
//! tool's run for a terminal.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use ttybench_core::budget::{self, Ab, Budget};
use ttybench_core::config::expand_home;
use ttybench_core::tools;

/// The options that the coordinator gives each tool itself, which a run's
/// arguments may not name.
pub const OWN_OPTIONS: [&str; 3] = ["--terminal", "--terminals", "--session"];

/// The folder of a session whose file names none, of the file's folder.
pub const DEFAULT_FOLDER: &str = "sessions/{date}";

/// A session, as its file describes it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Session {
    /// The terminals that it measures, by their names in the terminals'
    /// file, in their order of the first cycle.
    pub terminals: Vec<String>,
    /// The terminals' file, of the session file's folder; else the one
    /// that the tools look for.
    #[serde(default)]
    pub terminals_file: Option<String>,
    /// The session's folder, of the session file's folder, `{date}` the
    /// date and the time that it starts.
    #[serde(default)]
    pub folder: Option<String>,
    /// The seconds before each run.
    #[serde(default = "default_pause")]
    pub pause: f64,
    /// How many times the runs go, each time of another order.
    #[serde(default = "default_cycles")]
    pub cycles: u32,
    /// The rounds of each run, given as `--rounds`, unless the run says.
    #[serde(default)]
    pub rounds: Option<u32>,
    /// How many times each terminal launches before the first run, not
    /// counted, as a binary just built starts slower for a while.
    #[serde(default)]
    pub season: u32,
    /// The seconds that a run may take before it is stopped.
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    /// The runs, in their order of each cycle.
    #[serde(rename = "run", default)]
    pub runs: Vec<Run>,
    /// Two of its terminals compared, A before and B after, as two copies
    /// of one, which the report tells each metric's change of.
    #[serde(default)]
    pub ab: Option<Ab>,
    /// Its budgets, which the report tells held or not.
    #[serde(rename = "budget", default)]
    pub budgets: Vec<Budget>,
    /// Its style's file, of the session file's folder, of the report's
    /// and the charts' looks; the built-in style of none.
    #[serde(default)]
    pub style: Option<String>,
}

/// A tool that a session runs, of its arguments.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Run {
    /// The tool, as `startup` of `ttybench-startup`.
    pub tool: String,
    /// Its arguments, but those that the coordinator gives.
    #[serde(default)]
    pub args: Vec<String>,
    /// The terminals that it measures, of the session's; all of them of
    /// none.
    #[serde(default)]
    pub terminals: Option<Vec<String>>,
    /// Its rounds, in place of the session's.
    #[serde(default)]
    pub rounds: Option<u32>,
    /// Whether it needs `sudo`, which the coordinator asks for once and
    /// keeps.
    #[serde(default)]
    pub sudo: bool,
    /// The feature that it measures, as `smooth-scroll`, of the terminals
    /// that have it, on; the others skipped; of none, each terminal's base.
    #[serde(default)]
    pub feature: Option<String>,
}

/// A tool's run for a terminal, in a cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// Its id, of its cycle, its run, its tool and its terminal, which
    /// names its folder.
    pub id: String,
    /// Its cycle, from 1.
    pub cycle: u32,
    /// Its run, from 1.
    pub run: usize,
    /// Its terminal's place in its run's order of the cycle, from 1.
    pub position: usize,
    /// The tool.
    pub tool: String,
    /// The terminal.
    pub terminal: String,
    /// The tool's arguments, but those that the coordinator gives as it
    /// runs it.
    pub args: Vec<String>,
    /// Whether it needs `sudo`.
    pub sudo: bool,
    /// The feature that it measures, if one.
    pub feature: Option<String>,
}

fn default_pause() -> f64 {
    5.0
}

fn default_cycles() -> u32 {
    1
}

fn default_timeout() -> f64 {
    1800.0
}

impl Session {
    /// Why the session cannot run of the terminals `known`, if it cannot.
    pub fn check(&self, known: &BTreeSet<String>) -> Result<(), String> {
        names(&self.terminals, "the session")?;
        for name in &self.terminals {
            if !known.contains(name) {
                let known: Vec<&str> = known.iter().map(String::as_str).collect();
                return Err(format!(
                    "the terminals' file describes no terminal `{name}`, of {}",
                    known.join(", ")
                ));
            }
        }
        if self.runs.is_empty() {
            return Err("the session has no run: a [[run]] of a tool".to_owned());
        }
        if self.cycles == 0 {
            return Err("cycles is 1 at least".to_owned());
        }
        if !self.pause.is_finite() || self.pause < 0.0 {
            return Err(format!("pause is seconds, 0 or more, not {}", self.pause));
        }
        if !self.timeout.is_finite() || self.timeout <= 0.0 {
            return Err(format!(
                "timeout is seconds, more than 0, not {}",
                self.timeout
            ));
        }
        for (at, run) in self.runs.iter().enumerate() {
            let which = format!("run {} ({})", at + 1, run.tool);
            if !tools::is_tool_name(&run.tool) {
                return Err(format!("{which}: `{}` names no tool", run.tool));
            }
            if let Some(own) = run.args.iter().find(|arg| {
                OWN_OPTIONS
                    .iter()
                    .any(|option| arg == option || arg.starts_with(&format!("{option}=")))
            }) {
                return Err(format!(
                    "{which}: {own} is the coordinator's to give, of the session"
                ));
            }
            if let Some(terminals) = &run.terminals {
                names(terminals, &which)?;
                if let Some(other) = terminals.iter().find(|name| !self.terminals.contains(name)) {
                    return Err(format!(
                        "{which}: `{other}` is not among the session's terminals"
                    ));
                }
            }
        }
        budget::check(self.ab.as_ref(), &self.budgets, &self.terminals)
    }

    /// The terminals of `run`, in their order of the first cycle.
    fn terminals_of<'a>(&'a self, run: &'a Run) -> &'a [String] {
        run.terminals.as_deref().unwrap_or(&self.terminals)
    }

    /// The session's steps, in their order: each cycle's runs, each run's
    /// terminals in an order turned by one at each cycle, so that over as
    /// many cycles as a run has terminals each terminal takes each place
    /// once, first as often as last.
    pub fn steps(&self) -> Vec<Step> {
        let mut steps = Vec::new();
        for cycle in 1..=self.cycles {
            for (index, run) in self.runs.iter().enumerate() {
                let terminals = self.terminals_of(run);
                let turn = (cycle as usize - 1) % terminals.len().max(1);
                let order = terminals.iter().cycle().skip(turn).take(terminals.len());
                for (position, terminal) in order.enumerate() {
                    let mut args = run.args.clone();
                    if let Some(rounds) = run.rounds.or(self.rounds)
                        && !args.iter().any(|arg| arg == "--rounds")
                    {
                        args.extend(["--rounds".to_owned(), rounds.to_string()]);
                    }
                    if let Some(feature) = &run.feature
                        && !args.iter().any(|arg| arg == "--feature")
                    {
                        args.extend(["--feature".to_owned(), feature.clone()]);
                    }
                    steps.push(Step {
                        id: format!(
                            "c{cycle}-r{}-{}-{}",
                            index + 1,
                            run.tool,
                            file_name(terminal)
                        ),
                        cycle,
                        run: index + 1,
                        position: position + 1,
                        tool: run.tool.clone(),
                        terminal: terminal.clone(),
                        args,
                        sudo: run.sudo,
                        feature: run.feature.clone(),
                    });
                }
            }
        }
        steps
    }

    /// The runs whose orders the session's cycles leave unbalanced, as
    /// their number is not a multiple of their terminals'.
    pub fn unbalanced(&self) -> Vec<String> {
        self.runs
            .iter()
            .enumerate()
            .filter_map(|(at, run)| {
                let count = self.terminals_of(run).len();
                (count > 1 && !self.cycles.is_multiple_of(count as u32)).then(|| {
                    format!(
                        "run {} ({}): its {count} terminals' orders balance over cycles of a multiple of {count}, not {}",
                        at + 1,
                        run.tool,
                        self.cycles
                    )
                })
            })
            .collect()
    }

    /// The session's folder, of the file at `file`, at the date `date`.
    pub fn folder(&self, file: &Path, date: &str) -> PathBuf {
        let folder = self.folder.as_deref().unwrap_or(DEFAULT_FOLDER);
        relative_to(file, &expand_home(&folder.replace("{date}", date)))
    }

    /// The terminals' file that the session names, of the file at
    /// `file`, if it names one.
    pub fn terminals_path(&self, file: &Path) -> Option<PathBuf> {
        self.terminals_file
            .as_deref()
            .map(|path| relative_to(file, &expand_home(path)))
    }

    /// The style's file of the session of the file at `file`, if it names
    /// one.
    pub fn style_path(&self, file: &Path) -> Option<PathBuf> {
        self.style
            .as_deref()
            .map(|path| relative_to(file, &expand_home(path)))
    }
}

/// `path`, of the folder of the file at `file` where it is relative.
fn relative_to(file: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return path;
    }
    file.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or(path.clone(), |parent| parent.join(&path))
}

/// Why `names` are not a list of terminals, of `what`, if they are not.
fn names(names: &[String], what: &str) -> Result<(), String> {
    if names.is_empty() {
        return Err(format!("{what} names no terminal"));
    }
    let mut seen = BTreeSet::new();
    if let Some(twice) = names.iter().find(|name| !seen.insert(name.as_str())) {
        return Err(format!("{what} names `{twice}` twice"));
    }
    Ok(())
}

/// `name` as a part of a file's name: its letters, digits, `-` and `_`,
/// the rest `_`.
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttybench_core::config;

    fn session(text: &str) -> Session {
        config::parse(Path::new("session.toml"), text).unwrap()
    }

    #[test]
    fn a_session_s_style_is_of_its_file_s_folder() {
        let named =
            session("terminals = [\"a\"]\nstyle = \"look/style.toml\"\n[[run]]\ntool = \"idle\"\n");
        assert_eq!(
            named.style_path(Path::new("/bench/session.toml")),
            Some(PathBuf::from("/bench/look/style.toml"))
        );
        let none = session("terminals = [\"a\"]\n[[run]]\ntool = \"idle\"\n");
        assert_eq!(none.style_path(Path::new("/bench/session.toml")), None);
    }

    fn known() -> BTreeSet<String> {
        ["a", "b", "c"].map(String::from).into()
    }

    /// The order of each run's terminals in each cycle, of `steps`.
    fn orders(steps: &[Step]) -> Vec<(u32, usize, Vec<String>)> {
        let mut orders: Vec<(u32, usize, Vec<String>)> = Vec::new();
        for step in steps {
            match orders.last_mut() {
                Some((cycle, run, order)) if *cycle == step.cycle && *run == step.run => {
                    order.push(step.terminal.clone());
                }
                _ => orders.push((step.cycle, step.run, vec![step.terminal.clone()])),
            }
        }
        orders
    }

    #[test]
    fn a_runs_feature_goes_to_its_tool() {
        let session = session(
            "terminals = [\"a\", \"b\"]\n[[run]]\ntool = \"scroll\"\nfeature = \"smooth-scroll\"\n[[run]]\ntool = \"scroll\"\n",
        );
        let steps = session.steps();
        assert_eq!(steps[0].feature.as_deref(), Some("smooth-scroll"));
        assert_eq!(steps[0].args, ["--feature", "smooth-scroll"]);
        assert_eq!(steps[2].feature, None);
        assert!(steps[2].args.is_empty());
    }

    #[test]
    fn two_terminals_trade_places_at_each_cycle() {
        let session = session(
            "terminals = [\"a\", \"b\"]\ncycles = 2\n\
             [[run]]\ntool = \"startup\"\n[[run]]\ntool = \"idle\"\nargs = [\"--duration\", \"5\"]\n",
        );
        session.check(&known()).unwrap();
        assert!(session.unbalanced().is_empty());
        let steps = session.steps();
        let order = |names: [&str; 2]| names.map(String::from).to_vec();
        assert_eq!(
            orders(&steps),
            vec![
                (1, 1, order(["a", "b"])),
                (1, 2, order(["a", "b"])),
                (2, 1, order(["b", "a"])),
                (2, 2, order(["b", "a"])),
            ]
        );
        assert_eq!(steps[0].id, "c1-r1-startup-a");
        assert_eq!(steps[3].args, ["--duration", "5"]);
        assert_eq!(
            (
                steps[6].cycle,
                steps[6].position,
                steps[6].terminal.as_str()
            ),
            (2, 1, "b")
        );
    }

    #[test]
    fn each_terminal_takes_each_place_once_over_as_many_cycles() {
        let session =
            session("terminals = [\"a\", \"b\", \"c\"]\ncycles = 3\n[[run]]\ntool = \"memory\"\n");
        let steps = session.steps();
        for position in 1..=3 {
            let at: BTreeSet<&str> = steps
                .iter()
                .filter(|step| step.position == position)
                .map(|step| step.terminal.as_str())
                .collect();
            assert_eq!(at.len(), 3, "place {position}: {at:?}");
        }
        let two = Session {
            cycles: 2,
            ..session.clone()
        };
        assert_eq!(two.unbalanced().len(), 1);
    }

    #[test]
    fn a_run_of_some_terminals_turns_them_alone() {
        let session = session(
            "terminals = [\"a\", \"b\", \"c\"]\ncycles = 2\nrounds = 4\n\
             [[run]]\ntool = \"throughput\"\nterminals = [\"c\", \"a\"]\nrounds = 2\n\
             [[run]]\ntool = \"startup\"\nterminals = [\"b\"]\nargs = [\"--rounds\", \"9\"]\n",
        );
        session.check(&known()).unwrap();
        let steps = session.steps();
        let turned: Vec<(&str, &str)> = steps
            .iter()
            .map(|step| (step.tool.as_str(), step.terminal.as_str()))
            .collect();
        assert_eq!(
            turned,
            [
                ("throughput", "c"),
                ("throughput", "a"),
                ("startup", "b"),
                ("throughput", "a"),
                ("throughput", "c"),
                ("startup", "b"),
            ]
        );
        assert_eq!(steps[0].args, ["--rounds", "2"]);
        assert_eq!(steps[2].args, ["--rounds", "9"]);
    }

    #[test]
    fn the_example_runs_of_the_example_terminals() {
        let terminals: ttybench_core::config::Terminals = config::parse(
            Path::new("terminals.toml"),
            include_str!("../../../examples/terminals.toml"),
        )
        .unwrap();
        let known = terminals.terminals.keys().cloned().collect();
        let example = session(include_str!("../../../examples/session.toml"));
        example.check(&known).unwrap();
        assert_eq!(example.budgets.len(), 2);
    }

    #[test]
    fn a_session_that_cannot_run_is_refused() {
        let refused = [
            (
                "terminals = []\n[[run]]\ntool = \"startup\"\n",
                "names no terminal",
            ),
            (
                "terminals = [\"a\", \"a\"]\n[[run]]\ntool = \"startup\"\n",
                "twice",
            ),
            (
                "terminals = [\"z\"]\n[[run]]\ntool = \"startup\"\n",
                "no terminal `z`",
            ),
            ("terminals = [\"a\"]\n", "no run"),
            (
                "terminals = [\"a\"]\ncycles = 0\n[[run]]\ntool = \"startup\"\n",
                "cycles",
            ),
            (
                "terminals = [\"a\"]\npause = -1\n[[run]]\ntool = \"startup\"\n",
                "pause",
            ),
            (
                "terminals = [\"a\"]\n[[run]]\ntool = \"../x\"\n",
                "names no tool",
            ),
            (
                "terminals = [\"a\"]\n[[run]]\ntool = \"startup\"\nargs = [\"--session\", \"x\"]\n",
                "coordinator's",
            ),
            (
                "terminals = [\"a\"]\n[[run]]\ntool = \"startup\"\nargs = [\"--terminal=b\"]\n",
                "coordinator's",
            ),
            (
                "terminals = [\"a\"]\n[[run]]\ntool = \"startup\"\nterminals = [\"b\"]\n",
                "not among",
            ),
            (
                "terminals = [\"a\", \"b\"]\nab = { a = \"a\", b = \"c\" }\n[[run]]\ntool = \"startup\"\n",
                "the A/B names `c`",
            ),
            (
                "terminals = [\"a\"]\n[[run]]\ntool = \"startup\"\n[[budget]]\nmetric = \"ready_ms\"\nchange-at-most = 5\n",
                "of no A/B",
            ),
        ];
        for (text, why) in refused {
            let error = session(text).check(&known()).unwrap_err();
            assert!(error.contains(why), "{text:?}: {error}");
        }
        let error =
            config::parse::<Session>(Path::new("s.toml"), "terminals = [\"a\"]\ncycle = 2\n")
                .unwrap_err();
        assert_eq!(error.at.map(|at| at.0), Some(2));
    }

    #[test]
    fn the_folder_and_the_terminals_file_of_the_session_file() {
        let session = session(
            "terminals = [\"a\"]\nterminals-file = \"t.toml\"\n[[run]]\ntool = \"startup\"\n",
        );
        let file = Path::new("/bench/session.toml");
        assert_eq!(
            session.folder(file, "2026-11-23-1830"),
            Path::new("/bench/sessions/2026-11-23-1830")
        );
        assert_eq!(
            session.terminals_path(file),
            Some(PathBuf::from("/bench/t.toml"))
        );
        let named = Session {
            folder: Some("/tmp/s-{date}".to_owned()),
            ..session
        };
        assert_eq!(
            named.folder(Path::new("session.toml"), "d"),
            Path::new("/tmp/s-d")
        );
        assert_eq!(file_name("i term/2"), "i_term_2");
    }
}
