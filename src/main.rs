//! `ttybench`: benchmarks of terminal emulators. `ttybench <tool> …` runs
//! the tool's own binary, `ttybench-<tool> …`, found beside this one, then
//! on the `PATH`, as `git` runs its commands; `ttybench` alone lists them.

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

use ttybench::tools;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let dirs = tools::search_dirs(
        std::env::current_exe().ok().as_deref(),
        std::env::var_os("PATH").as_deref(),
    );
    let Some(first) = args.next() else {
        print!("{}", usage(&dirs));
        return ExitCode::SUCCESS;
    };
    match first.to_str() {
        Some("-h" | "--help" | "help") => {
            print!("{}", usage(&dirs));
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("ttybench {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(name) if tools::is_tool_name(name) => dispatch(name, args.collect(), &dirs),
        _ => {
            eprintln!(
                "ttybench: `{}` is no tool's name: `ttybench --help` lists them",
                first.to_string_lossy()
            );
            ExitCode::from(2)
        }
    }
}

/// Runs the tool `name` of `args` in this process's place, its exit status
/// this one's; 127 of a tool not found, 126 of one that does not run.
fn dispatch(name: &str, args: Vec<OsString>, dirs: &[std::path::PathBuf]) -> ExitCode {
    let Some(path) = tools::find(name, dirs) else {
        eprintln!(
            "ttybench: there is no tool `{name}`, a binary `{}{name}` beside ttybench or on the PATH: `ttybench --help` lists them",
            tools::PREFIX
        );
        return ExitCode::from(127);
    };
    let error = Command::new(&path).args(args).exec();
    eprintln!("ttybench: {} does not run: {error}", path.display());
    ExitCode::from(126)
}

/// What `ttybench --help` prints: how it runs, and the tools of `dirs`.
fn usage(dirs: &[std::path::PathBuf]) -> String {
    let mut text = format!(
        "ttybench {}: benchmarks of terminal emulators\n\nUsage: ttybench <tool> [arguments…]\n       ttybench --help | --version\n\nEach tool is a binary of its own, `{}<tool>`, beside ttybench or on the PATH.\n\n",
        env!("CARGO_PKG_VERSION"),
        tools::PREFIX
    );
    let found = tools::list(dirs);
    if found.is_empty() {
        text.push_str("No tool found.\n");
    } else {
        text.push_str("Tools:\n");
        for tool in found {
            text.push_str(&format!("  {:<14} {}\n", tool.name, tool.path.display()));
        }
    }
    text
}
