# ttybench

Benchmarks of terminal emulators on macOS: how fast a terminal starts, shows a key you type, prints, scrolls and rests, how much memory and energy it takes, and how it uses the GPU.

ttybench knows no terminal by name. You describe the terminals you want to measure in a TOML file of your own: how each one launches with a command, and the environment it gets. Every tool then measures a terminal through its process: its window, the keys sent to it, the capture of what it shows, and the CPU, memory, energy and GPU time of its processes.

Each tool is a binary of its own, `ttybench-<tool>`, which you can run alone. `ttybench <tool>` runs it too, as `git` runs its commands, and `ttybench --help` lists the tools it finds beside it or on your `PATH`.

> [!NOTE]
> ttybench is under way: the tools come one by one, toward version 0.1.0.

## Describe your terminals

Copy [`examples/terminals.toml`](examples/terminals.toml) and keep the terminals you have. Each one says how it launches with a command:

```toml
[terminals.ghostty]
app = "/Applications/Ghostty.app"
launch = ["{app}/Contents/MacOS/ghostty", "-e", "{command}"]

[terminals.iterm2]
app = "/Applications/iTerm.app"
launch = "open"
```

- `launch` is the program and its arguments, with `{app}` for the bundle, `{command}` for the script that ttybench runs in the terminal, and `{cwd}` for the folder it starts in. A terminal that takes no command on its command line, such as Terminal.app or iTerm2, uses `"open"`: ttybench opens a `.command` script with the app.
- `env` gives the terminal more environment variables, such as the folder of a config made for the benchmarks.
- Give every terminal the same font, font size and window size, so that their figures compare.

The tools read `--terminals <file>`, else `$TTYBENCH_TERMINALS`, else `terminals.toml` in the current folder, else `~/.config/ttybench/terminals.toml`.

## The tools

### startup

How fast a terminal starts: from its launch to its window on screen, and to its shell running the command.

```sh
ttybench startup --terminal ghostty --rounds 10 --pause 2
```

Each round launches the terminal running `ttybench-payload`, waits for its window and for the payload to report that it runs, then quits the terminal. `--season <n>` runs that many rounds first without counting them, as a binary just built starts slower for its first launches. The records go to `--session <folder>`, `ttybench-session` by default.

## License

MIT, as [LICENSE](LICENSE) says.

### idle

What a terminal uses at rest: the share of a CPU that its processes take, how often they wake up a CPU package or take an interrupt, and the power that macOS bills them. A terminal at rest should take almost none.

```sh
ttybench idle --terminal ghostty --duration 10 --interval 1
```

Each round launches the terminal, lets it settle for `--settle` seconds, then reads its processes' counters every `--interval` for `--duration`. `--workload "<workload>"` runs a workload of `ttybench-payload` instead of a rest, such as `"lines --count 100000"`. It needs no `sudo`.

### memory

How much memory a terminal takes: the physical footprint of its processes, as Activity Monitor shows Memory, summed, now and at their peaks.

```sh
ttybench memory --terminal ghostty --rounds 3
```

