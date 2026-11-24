# ttybench

Benchmarks of terminal emulators on macOS: how fast a terminal starts, shows a key you type, prints, scrolls and rests, how much memory and energy it takes, and how it uses the GPU.

ttybench knows no terminal by name. You describe the terminals you want to measure in a TOML file of your own: how each one launches with a command, and the environment it gets. Every tool then measures a terminal through its process: its window, the keys sent to it, the capture of what it shows, and the CPU, memory, energy and GPU time of its processes.

Each tool is a binary of its own, `ttybench-<tool>`, which you can run alone. `ttybench <tool>` runs it too, as `git` runs its commands, and `ttybench --help` lists the tools it finds beside it or on your `PATH`.

This is version 0.1.0.

- [What it measures](#what-it-measures)
- [Install](#install) and [permissions](#permissions)
- [A first session](#a-first-session)
- [Measure honestly](#measure-honestly)
- [Describe your terminals](#describe-your-terminals), [run a session](#run-a-session), [read its report](#read-a-sessions-report), [see its charts](#see-a-sessions-charts)
- [The tools](#the-tools), one by one
- [Limits](#limits)

## What it measures

| Tool | What | Its metrics | It takes |
|---|---|---|---|
| [`startup`](#startup) | From the launch to the window, and to the shell running a command | `window_ms`, `ready_ms` | |
| [`idle`](#idle) | The CPU, the wakeups and the power of a terminal at rest | `cpu_percent`, `package_idle_wakeups_per_s`, `interrupt_wakeups_per_s`, `power_mw` | |
| [`memory`](#memory) | The memory of its processes | `footprint_mib`, `peak_mib` | |
| [`throughput`](#throughput) | How fast it prints a file | `written_mib_s`, `quiet_mib_s` | |
| [`latency`](#latency) | From a key's press to the frame that shows it, beside a flood too | `latency_ms`, `missed_keys` | Screen Recording, Accessibility |
| [`scroll`](#scroll) | The frames that show the text moved as you scroll | `frames_per_s`, `frame_gap_ms` | Screen Recording, Accessibility |
| [`gpu`](#gpu) | Its GPU time and WindowServer's, and the system's GPU | `terminal_gpu_ms_per_s`, `window_server_gpu_ms_per_s`, `gpu_power_mw`, `gpu_active_percent`, `gpu_freq_mhz` | Accessibility to scroll, `sudo` for the system's |
| [`frames`](#frames) | Each frame on the GPU and on the display | `frame_gpu_ms`, `frames_per_s`, `frame_gap_ms`, `refreshes_with_frame`, `frames_shown_per_s`, `frames_dropped` | Xcode, Accessibility to scroll |
| [`motion`](#motion) | Its window dragged or resized: frames, misses, the compositor's delay | `frames_per_s`, `missed_frames`, `compositor_delay_ms`, `terminal_cpu_percent`, `window_server_cpu_percent` | Xcode, Accessibility |

Beside them, [`run`](#run-a-session) runs a session of tools and terminals in balanced orders, [`report`](#read-a-sessions-report) sums it up in tables, its A/B and its budgets, and [`charts`](#see-a-sessions-charts) draws it, with a page of both. `ttybench-payload` is what runs inside the terminal: it reports that it runs, prints, reads keys and rests, and you never run it yourself.

## Install

ttybench runs on a Mac of Apple Silicon, of macOS 26 or later. It builds with Rust, of the version that `rust-toolchain.toml` pins, which [rustup](https://rustup.rs) installs by itself:

```sh
git clone https://github.com/sephiroth74/ttybench.git
cd ttybench
cargo build --release --locked
export PATH="$PWD/target/release:$PATH"   # and in your shell's profile
ttybench --help
```

`frames` and `motion` record Instruments' traces with `xctrace`, which comes with Xcode, not with its Command Line Tools: install Xcode, and point `xcode-select` at it once, `sudo xcode-select --switch /Applications/Xcode.app`.

## Permissions

macOS asks for a permission of the app that runs ttybench, such as your terminal or your editor, not of ttybench itself: grant it in System Settings › Privacy & Security, then quit and open that app again.

- **Accessibility**, for the keys, the clicks, the scrolls and the drags that `latency`, `scroll`, `motion`, and `gpu` and `frames` of a scroll send. Without it, macOS drops them and tells nothing: the tools check first and stop.
- **Screen & System Audio Recording**, for the capture of the window that `latency` and `scroll` read.
- **`sudo`**, for `gpu --system`, which reads `powermetrics`: run `sudo -v` first, or give its run `sudo = true` in a session, which asks once and keeps it.

Run ttybench from an app that you don't measure: each tool launches a terminal of its own and quits it once measured, and a copy of the same terminal already open shares the machine with the measure. `ttybench run` tells you when one is.

## A first session

1. **Give the terminals one config.** Copy [`examples/configs`](examples/configs) to `~/ttybench/configs`: Ghostty, kitty, Alacritty and Lokyr, each of Menlo at 13 points, 120 × 40 cells, no ligatures, a cursor that doesn't blink and an opaque window. A terminal that reads its config from the folder that `XDG_CONFIG_HOME` names takes it from there; give the others' profiles the same by hand.

   ```sh
   mkdir -p ~/ttybench && cp -R examples/configs ~/ttybench/configs
   ```

2. **Describe your terminals.** Copy [`examples/terminals.toml`](examples/terminals.toml) to `~/.config/ttybench/terminals.toml`, and keep the terminals that you have:

   ```sh
   mkdir -p ~/.config/ttybench && cp examples/terminals.toml ~/.config/ttybench/
   ```

3. **Try a tool on one terminal**, three rounds of its startup, and read what it found:

   ```sh
   ttybench startup --terminal ghostty --rounds 3 --session /tmp/first
   ttybench report /tmp/first --out -
   ```

4. **Write a session**: two terminals, their startup and their throughput, two cycles, so that each runs first once:

   ```toml
   # session.toml
   terminals = ["ghostty", "kitty"]
   pause = 2
   cycles = 2
   season = 10

   [[run]]
   tool = "startup"
   rounds = 10

   [[run]]
   tool = "throughput"
   args = ["--text", "plain"]
   rounds = 3
   ```

5. **Run it**, after a look at its runs in their order, and leave the Mac alone while it runs:

   ```sh
   ttybench run session.toml --dry-run
   ttybench run session.toml
   ```

6. **Open its page**, `sessions/<date>/index.html`, of the report's tables and the charts; `report.md` holds the same tables.

Then add `latency` and `scroll` once the app that runs ttybench has the Screen Recording and Accessibility permissions, and `gpu --system` of `sudo = true`.

## Measure honestly

A figure tells a terminal apart only where everything else stays alike. ttybench keeps what it can alike, and records what it can't, so that the report compares records of the same conditions alone; the rest is yours.

- **One config for every terminal**: the same font, font size and window size, a cursor that doesn't blink, no ligatures, an opaque window, no blur, no plugins or extensions, no shell integration, and every feature off that another terminal lacks. A feature is measured of the terminals that have it, apart, as [Features](#features) tells.
- **One session**: the machine's load, its temperature, its display and its power move the figures from a session to the next by more than many changes. Compare terminals, and copies of one, within a session, and see a history of sessions as a trend.
- **Balanced orders**: a session turns each run's terminals at each cycle, so that each runs first as often as last; give it cycles of a multiple of the terminals' number.
- **The same pause** before each run, so that each starts of the same rest.
- **Seasoned copies**: a terminal just built, installed or moved starts slower for its first launches, while macOS checks and caches it; `season` launches each terminal that many times before the first run. Build two copies of one terminal alike: a copy that is not notarized starts slower than one that is.
- **The Mac left alone**: no other app moving on screen, the pointer and the keyboard untouched, the Mac on its power adapter. A background scan of new files, as an antivirus's of a trace just written, takes the machine for minutes.
- **The same display** for every terminal of a measure, at its rate: each record keeps the display's refresh rate.

## Describe your terminals

Copy [`examples/terminals.toml`](examples/terminals.toml) and keep the terminals you have. Each one says how it launches with a command:

```toml
[terminals.ghostty]
app = "/Applications/Ghostty.app"
launch = ["{app}/Contents/MacOS/ghostty", "-e", "{command}"]
env = { XDG_CONFIG_HOME = "~/ttybench/configs" }

[terminals.iterm2]
app = "/Applications/iTerm.app"
launch = "open"
```

| Key | What |
|---|---|
| `app` | The app's bundle, a path absolute or from `~`. Its version, and its build where that differs, go into each record. |
| `launch` | The program and its arguments, of `{app}` for the bundle, `{command}` for the script that ttybench runs in the terminal, and `{cwd}` for the folder it starts in. A terminal that takes no command on its command line, as Terminal.app, iTerm2 or Warp, uses `"open"`: ttybench opens a `.command` script with the app, through `open -n -a`. |
| `env` | More environment variables for the terminal, as `XDG_CONFIG_HOME`, the folder of its config of the benchmarks; `~` expands. |
| `quit` | How it quits once measured: `"signal"`, the default, a `SIGTERM` to its processes, or `"app"`, its own quit, as its menu's; a `SIGKILL` past a deadline either way. |
| `beside` | The keys that open a pane beside the measured one and give it the keyboard, `open`, and those that give it back, `back`, or where to click, `click`, for [`latency --flood`](#latency). |
| `features.<name>` | A feature that the terminal has, and how it turns on: `env` in place of the base's variables, `args` after its program's name. See [Features](#features). |

The tools read `--terminals <file>`, else `$TTYBENCH_TERMINALS`, else `terminals.toml` in the current folder, else `~/.config/ttybench/terminals.toml`. A key is written as `cmd+shift+d`: its modifiers, `cmd`, `ctrl`, `alt` and `shift`, then a character of the US layout, or `enter`, `tab`, `space`, `escape`, `backspace`, `delete`, an arrow, `home`, `end`, `pageup`, `pagedown` or `f1` to `f8`.

### Measure the same thing of every terminal

A measure compares terminals only when each runs of the same conditions: give every terminal a base config alike, of the same font, font size and window size, a cursor that doesn't blink, no ligatures, an opaque window, no plugins or extensions of its own, and every feature that another terminal may lack turned off. ttybench can't read two terminals' configs and tell them alike: that is yours to do, in the configs that your terminals' file points them at. Each record keeps the feature that it measured, if one, and what the tools see of the conditions, as the window's cells, so that reports compare records of the same conditions alone, and tell apart those that differ.

### Features

Some measures compare a feature that not every terminal has, such as smooth scrolling. Describe each terminal's features and how each turns on, and give it a base config of every feature off:

```toml
[terminals.lokyr]
app = "/Applications/Lokyr.app"
launch = "open"
env = { XDG_CONFIG_HOME = "~/ttybench/configs" }                       # smooth scrolling off
features.smooth-scroll = { env = { XDG_CONFIG_HOME = "~/ttybench/configs-smooth" } }

[terminals.kitty]
app = "/Applications/kitty.app"
launch = ["{app}/Contents/MacOS/kitty", "{command}"]
features.some-feature = { args = ["-o", "some_option yes"] }
```

- A feature's `env` replaces the base's variables of the same names, such as the folder of a config of the feature on. Its `args` go right after the terminal's program, for a terminal launched by its arguments.
- Every tool takes `--feature <name>`: it measures a terminal of that feature on, and skips a terminal that doesn't have it, of the exit status 3, which a session tells as skipped, not failed. Without `--feature`, the tools measure each terminal's base.
- So that two terminals compare, measure the same thing of both: a terminal's smooth scrolling beside another's scroll by rows measures two features, not two terminals.

## Run a session

`ttybench run` runs a session that a file describes: the terminals, the tools and their arguments, and how many cycles. Copy [`examples/session.toml`](examples/session.toml):

```toml
terminals = ["ghostty", "kitty"]
pause = 5        # seconds before each run
cycles = 2       # each of another order

[[run]]
tool = "startup"
rounds = 10

[[run]]
tool = "throughput"
args = ["--text", "plain"]
```

```sh
ttybench run session.toml --dry-run   # the runs, in their order
ttybench run session.toml
```

| Key | What | By default |
|---|---|---|
| `terminals` | The terminals, by their names in the terminals' file, in their order of the first cycle | |
| `terminals-file` | The terminals' file, from the session file's folder | the one that the tools look for |
| `folder` | The session's folder, from the session file's folder; `{date}` is the date and time it starts | `sessions/{date}` |
| `pause` | The seconds before each run | 5 |
| `cycles` | How many times the runs go, each time in another order | 1 |
| `rounds` | Each run's `--rounds`, unless the run says | the tool's |
| `season` | How many times each terminal launches before the first run, not counted | 0 |
| `timeout` | The seconds that a run may take before it is stopped | 1800 |
| `ab` | Two terminals compared, `{ a = "<before>", b = "<after>" }`: see [Compare two copies](#compare-two-copies-of-a-terminal) | |
| `[[run]]` | A tool to run: `tool`, its `args` but `--terminal`, `--terminals` and `--session`, which the session gives, its `terminals` of the session's, its `rounds`, `sudo = true`, and the `feature` that it measures | |
| `[[budget]]` | A limit of a metric: see [Budgets](#budgets) | |

- **Balanced orders**: each cycle turns each run's terminals by one, so that over as many cycles as a run has terminals each one runs first as often as last. ttybench tells you when the cycles don't balance them.
- **The session's folder**, `sessions/<date>` by default, keeps a copy of the session's and the terminals' files, `machine.json`, `runs.jsonl` of each run's end, `records.jsonl` of every record, each of its cycle, and a folder of each run of its output.
- **A run that fails** is told, with its output's last lines, and the session goes on. **A run skipped**, of a feature that its terminal doesn't have, is told apart, and `--resume` doesn't run it again. `ttybench run --resume <folder>` takes up a session that was cut off, or runs its failed runs again; what ended well stays as it is.
- **`feature`** on a run measures the terminals that have that feature, of it on, and skips the others, without launching them: `--dry-run` tells them, and the session's log keeps them as skipped. A run of no `feature` measures each terminal's base.
- **`season`** launches each terminal that many times before the first run, not counted, as a terminal just built or installed starts slower for its first launches.
- **`sudo = true`** on a run that needs `sudo`: the session asks for it once and keeps it.
- **The report and the charts**: at its end, the session writes its report and its charts into its folder, as the next sections tell, unless `--no-report`.

## Read a session's report

`ttybench report` sums up a session's folder: `report.md` for you to read, and `report.json` of the same figures for the charts and your own tools. The coordinator writes both at a session's end; run it alone on any session's folder, a tool's alone too:

```sh
ttybench report sessions/2026-11-24-1405
ttybench report sessions/2026-11-24-1405 --out -   # the Markdown to the terminal, no file written
```

- **A table of each metric** of each tool, of the records of the same conditions alone: each terminal's median and the 95 % bootstrap interval of the median, the 95th percentile, the mean and the standard deviation, the least and the most, the samples and the runs that gave them, and each cycle's median. A run of a `feature` makes its own tables, which name the feature, and its terminals skipped are told in them.
- **Intervals of the runs**: a run moves as a whole of the machine's state, as another program's work beside it raises the processor's clocks, so the intervals resample a terminal's runs first, then each run's samples. A median of one run tells its samples' interval alone.
- **The runs' table**: each run's start, length and end, the load average of a minute as it started and as it ended, and the busiest processes as it ended, which tell a run measured beside another program's work, as a scan of the files.
- **The runs that measured nothing**, failed or skipped of no table to tell them in, close the report, of their reason.

### Compare two copies of a terminal

An A/B compares two terminals of one session, as a release and its next build, each described in your terminals' file of its own app:

```toml
terminals = ["lokyr-1.4", "lokyr-1.5"]
ab = { a = "lokyr-1.4", b = "lokyr-1.5" }   # A before, B after
```

The report's A/B table gives each metric's median of A and of B, the change, B's less A's, in the unit and in percent, and its 95 % bootstrap interval, of each copy's runs resampled, then their samples: a change whose interval holds 0 is within the noise; one beyond it is better or worse, where the metric has a better way, or higher or lower; a copy of fewer than two runs tells too few. Give an A/B several cycles: each copy's runs, interleaved with the other's, meet the machine's states alike. `ttybench report <folder> --a <name> --b <name>` compares two others.

Measure both copies in one session: two sessions' figures differ of the machine's state more than many changes do.

### Budgets

A budget is a limit of a metric, which the report tells held or not:

```toml
[[budget]]
tool = "startup"
metric = "ready_ms"
at-most = 300                # every terminal's median, in the metric's unit

[[budget]]
metric = "latency_ms"
terminal = "lokyr-1.5"
stat = "p99"                 # median, mean, p95, p99, min or max; the median by default
at-most = 75

[[budget]]
metric = "written_mib_s"
conditions = { text = "plain" }
change-at-least = "-5%"      # B's median at most 5 % below A's
```

- **`metric`** names the metric, as the tables' small line shows it; `tool` and `conditions` narrow the records that it judges, each condition of the value that the records' has; `terminal` narrows the terminals.
- **`at-most` and `at-least`** limit each terminal's statistic. **`change-at-most` and `change-at-least`** limit its change from the A/B's A to its B, in the metric's unit, or in percent of A's, as `"5%"`: they need an `ab`.
- `ttybench run` checks the budgets as it starts, so that a typo doesn't wait for the session's end.

## See a session's charts

`ttybench charts` draws a session's charts as SVG files, from its `report.json` and its records, into its folder's `charts/`, which the coordinator writes after the report; run it alone after `ttybench report`:

```sh
ttybench charts sessions/2026-11-24-1405
ttybench charts sessions/2026-11-24-1405 --out ~/Desktop/charts
```

- **Bars** of each terminal's median of each metric, and a ring of its 95th percentile, with a tick of each budget that judges it.
- **The latency's distribution**: the share of the keys echoed within each time, every key, then the slowest tenth, where the tails differ.
- **The gaps between frames in time**, of the scroll's and the frames' tools, a panel of each terminal on one scale, with a line of a refresh's length.
- **The GPU's time** of each terminal, its own then WindowServer's, stacked.
- **The A/B**: each metric's change of the median in percent of A's, its 95 % interval, better, worse or within the noise, and the budgets of the change.

Each terminal keeps its color in every chart, by its place in the session. The charts follow the light or dark appearance of the page or system that shows them, or a page's `light` or `dark` class on the chart. Each chart has a title and a description for screen readers, and tells its values in a tooltip as the pointer rests on a mark. `charts/charts.json` lists the charts, their titles and the report's groups that each shows.

Last, `ttybench charts` writes the session's page, `index.html` in its folder: the report's tables, each chart after its own table. It is one static file of no script, its charts inline, so it opens in any browser, without a network, and can be sent as it is.

### A terminal along its versions

`--history` draws the history of several sessions, the oldest first: for each terminal of two sessions or more, a chart of each tool, a panel of each metric, each session's median a dot, along the terminal's versions:

```sh
ttybench charts --history sessions/1.4.0 sessions/1.5.0 --out history
```

A version and its build both show, as `1.4.0 (202611220721)`, so that two builds of one version tell apart. A metric joins its own across sessions of the same conditions, whatever their rounds, pauses and seasoning; a session of other conditions, as another window size, starts a panel of its own. `history/index.html` shows them all.

Sessions measure alike within a few percent alone: the machine's load, its display and its temperature move a session's figures. The history tells a trend; an A/B in one session tells a change.

## The tools

### startup

How fast a terminal starts: from its launch to its window on screen, and to its shell running the command.

```sh
ttybench startup --terminal ghostty --rounds 10 --pause 2
```

Each round launches the terminal running `ttybench-payload`, waits for its window and for the payload to report that it runs, then quits the terminal. `--season <n>` runs that many rounds first without counting them, as a binary just built starts slower for its first launches. The records go to `--session <folder>`, `ttybench-session` by default.

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

### throughput

How fast a terminal prints: the MiB a second of a file that it prints, of plain text, of a color before every character, or of wide characters that take two cells each.

```sh
ttybench throughput --terminal ghostty --text plain --mib 256 --rounds 5
```

Each round launches the terminal running `ttybench-payload`, which prints a file of `--mib` MiB, 256 by default, as fast as the terminal takes it: a smaller file leaves more of its time to the window's first frames. Two rates come of each round: to the last byte's write, as the terminal took the bytes, and to the quiet, as its processes' CPU drops below 5 % of a core for 60 ms, once it has drawn what it took. `--text` is `plain`, `colors` or `wide`. Each text's file of each size is written once, into the temporary folder's `ttybench-cache`, and printed again by every run after: a session writes no GiB a run, which the system's file watchers, as an antivirus's, would take the machine for while it measures.

### latency

How fast a terminal shows a key that you type: from the key's press to the frame that shows its character on screen.

```sh
ttybench latency --terminal ghostty --keys 200 --spacing 150
ttybench latency --terminal ghostty --keys 200 --flood
```

Each round launches the terminal running `ttybench-payload`, which reads keys on the first row of a cleared screen that the terminal echoes them on, as `cat` does at a prompt, and clears it every 40 keys. ttybench captures the terminal's window with ScreenCaptureKit, strikes a few keys slowly to find the row and its cells, then strikes `.` `--keys` times, `--spacing` milliseconds apart, as Typometer strikes it. A key's latency ends at the first frame that shows its row changed, at the time the window server took that frame, so that the capture's own delay doesn't count.

- **Every phase of the refresh**: a frame shows at the display's next refresh after the terminal draws it, so a key's latency depends on when, within a refresh, the key comes. ttybench adds a random share of a refresh to each spacing, so that the keys fall at every phase alike and the figures hold the whole range. `--sync` strikes each key a spacing after the frame of the last one's character instead, as Typometer's synchronous mode does: with a spacing of whole refreshes, as 150 ms is at 60 and 120 Hz, every key comes at the same phase, and the figures hold that phase alone.
- **Beside a flood**: `--flood` opens a pane beside the measured one with the terminal's `beside` keys, runs `yes` in it, and measures the keys while it prints. Describe `beside` in your terminals' file: the keys that open the pane and give it the keyboard, and the keys that give the keyboard back to the measured pane, or where to click in it, as shares of the window from its top left:

  ```toml
  [terminals.ghostty]
  beside = { open = ["cmd+d"], back = ["cmd+["] }

  [terminals.kitty]
  beside = { open = ["cmd+enter"], click = [0.5, 0.25] }
  ```

  ttybench sends a shortcut's character with its key, so that an app that matches its shortcuts by their characters, as a menu does, finds them whatever your keyboard's layout. An app that reads the key's place on your layout may not: give it a click.
- **The cursor must not blink**: ttybench watches the row for a second before the keys, and stops when it changes of its own.
- **Where the keys go**: `--to process`, the default, sends them to the terminal's process alone, whichever app is in front. `--to system` sends them through the window server, as a keyboard does, to the app in front, which the terminal is as it launches. A click always goes through the window server: leave the Mac alone meanwhile.
- **The display kept awake**: macOS counts ttybench's keys, clicks and scrolls as no one's activity, and would let the display sleep in the middle of a session, which leaves nothing to capture. ttybench holds the display awake while it measures, as `caffeinate -d` does; `--awake off` doesn't. It declares no activity of yours, which would hold the window server's compositor a refresh longer on each frame.
- **Your Mac's state counts**: the same terminal can show a latency some milliseconds lower while you work on the Mac than while it rests. Measure every terminal of a comparison in the same state, and in one session.
- **What the capture takes**: `--source window`, the default, takes the window alone, each frame as the window server takes the window's new content. `--source screen` takes the screen where the window lies, each frame as the window server composes it, a few milliseconds later, with whatever lies over the window. While ttybench captures a window, macOS shows its capture indicator in the place of the window's buttons, away from the typed row.
- **Permissions**: the capture takes the Screen Recording permission, and the keys the Accessibility permission, of the app that runs ttybench, such as your terminal or your editor. Without the latter, macOS drops the keys and tells nothing, so ttybench stops before it measures.

#### Beside Typometer

Typometer 1.1.0 measures a key's latency from the screen with Java's Robot, on macOS:

- **It reads one pixel at a time**, where the next character's `.` goes, or, of a block cursor, where the cursor goes next, again and again until it changes. Each read takes about a refresh, 18 ms at 60 Hz, so it sees a change once a refresh at most, and keeps a processor's core busy meanwhile.
- **Its synchronous mode** strikes the next key 150 ms after it saw the last character: whole refreshes at 60 and 120 Hz, so every key comes at about the same phase of the refresh, and its figures hold that phase alone, which may lie a refresh from another's. **Its asynchronous mode** strikes keys at a steady pace: of a delay of no whole refreshes, as 153 ms, they fall at every phase, as ttybench's do.
- **Its native API** is Windows' and Linux's: on macOS it reads through Robot either way.

Both measured two terminals in one session, on a display at 60 Hz, the Mac left alone, each terminal of Menlo at 13 points in 120 by 40 cells and a cursor that doesn't blink, 200 keys each: Typometer's asynchronous mode at 153 ms beside ttybench's every phase, its synchronous mode at 150 ms beside `--sync`, and both beside a pane of `yes`. The median and the 95th percentile, by the nearest rank, in milliseconds:

| Terminal | Measure | Typometer | ttybench |
|---|---|---|---|
| Ghostty 1.4.0 | every phase | 19.6 · 27.1 | 20.2 · 37.4 |
| Ghostty 1.4.0 | one phase | 24.9 · 30.6 | 16.1 · 34.3 |
| Ghostty 1.4.0 | beside a flood | 32.6 · 42.0 | 32.2 · 38.8 |
| kitty 0.51.0 | every phase | 20.3 · 26.7 | 18.9 · 28.2 |
| kitty 0.51.0 | one phase | 23.9 · 30.9 | 16.1 · 18.3 |
| kitty 0.51.0 | beside a flood | 163.1 · 287.1 | 99.1 · 304.2 |

- **Of every phase**, the medians lie within 1.5 ms of each other: both measure the same.
- **Of one phase**, they lie 8 to 9 ms apart, within a refresh of 16.7 ms: each holds a phase of its own, which no setting of either chooses.
- **Beside a flood**, Ghostty draws without a pause, and the phase matters little: within half a millisecond. kitty's latency beside a flood depends on how much of the flood it has still to take, and spreads from 16 to 520 ms: the medians of 200 keys lie 64 ms apart, the 95th percentiles 17.
- ttybench then struck letters on the window's middle row; it strikes `.` on the first row since, which a check of Ghostty's every phase measured 2.5 ms below Typometer's.

### scroll

How smoothly a terminal scrolls: the frames a second that it shows while you scroll, and how steady they come.

```sh
ttybench scroll --terminal ghostty --by pixels --seconds 10
ttybench scroll --terminal ghostty --by lines
```

Each round launches the terminal running `ttybench-payload`, which prints 50 000 numbered lines, then scrolls its window and captures it, counting the frames that show the text moved, and the gaps between them.

- **`--by pixels`**, the default, scrolls as a trackpad does: each period of 2 seconds swipes up `--amplitude` points, 200 by default, over half a second, `--rate` events a second, 120 by default, then lets its momentum slow over the next half second, then the same down, so that the view moves throughout. **`--by lines`** turns a mouse wheel a step at a time, 20 steps a second, up for a second, then down.
- **The frames are those that the window server took**, as it took them. Which of them each refresh of the display showed is the display's own record: a terminal that draws in step with the display's refreshes gives frames a whole number of refreshes apart, one that draws at a pace of its own gives frames between them, of which a refresh shows the last.
- **Leave the mouse and the trackpad alone**: the scroll goes through the window server to the window under the pointer, which ttybench moves to the window's middle. macOS keeps the app that someone uses in front as a terminal launches: ttybench brings the terminal's app to the front through the Accessibility permission where another app's window covers it, and stops, of its reason, where it can't. `motion`, and `gpu` and `frames` of a scroll do alike.
- **`--source` and `--awake`** are latency's.

### gpu

How much of the GPU a terminal uses: its processes' GPU time and WindowServer's, at rest or while it scrolls, and, of `--system`, the system's GPU.

```sh
ttybench gpu --terminal ghostty --workload rest --seconds 10
ttybench gpu --terminal ghostty --workload scroll --seconds 10 --system
```

Each round launches the terminal running `ttybench-payload`, then reads the GPU over `--seconds` of rest, or of the same scroll as `ttybench scroll`'s (`--workload scroll`, `--by pixels|lines`).

- **The processes' GPU time**, in milliseconds a second: the terminal's processes', and WindowServer's, which composes every window on the display, their translucency, blur and layers, and whose work is billed to it, not to the app that asked for it. It comes of the GPU's clients that IOKit lists, as Activity Monitor shows a process's GPU time, and takes no privilege. WindowServer's holds what every app on screen asks of it: measure with nothing else moving.
- **`--system`** samples the system's GPU with `powermetrics` every `--interval` milliseconds, 250 by default: its power in milliwatts, the share of the time that it worked, and its frequency while it worked. It takes `sudo`, of credentials already given: run `sudo -v` first, or a session's run of `sudo = true`.
- **The two agree**: the terminal's and WindowServer's GPU time make about the share of the time that the system's GPU worked, as the other apps ask little while you measure.

Two terminals at 60 Hz, 10 seconds each, a display of nothing else moving:

| Terminal | Workload | Its GPU time | WindowServer's | GPU power | GPU active |
|---|---|---|---|---|---|
| Ghostty 1.4.0 | rest | 1.73 ms/s | 2.27 ms/s | 1.9 mW | 1.0 % |
| Ghostty 1.4.0 | scroll | 47.4 ms/s | 44.4 ms/s | 32.3 mW | 8.5 % |
| kitty 0.51.0 | rest | 0.00 ms/s | 1.04 ms/s | 1.0 mW | 0.6 % |
| kitty 0.51.0 | scroll | 56.2 ms/s | 48.6 ms/s | 41.1 mW | 10.0 % |

### frames

Each frame of a terminal on the GPU, while it scrolls or rests: its GPU time, the frames a second, the gaps between them, and how the display showed them.

```sh
ttybench frames --terminal ghostty --workload scroll --seconds 10
ttybench frames --terminal lokyr --feature smooth-scroll
```

Each round launches the terminal running `ttybench-payload`, records Instruments' Metal System Trace of it with `xctrace`, Xcode's, over `--seconds` of the same scroll as `ttybench scroll`'s, or of rest, then reads the trace and removes it.

- **A frame's GPU time** is the time that the GPU worked on it, of its vertex and fragment work, which may overlap, in milliseconds.
- **The frames a second** are those that the terminal drew on the GPU. A terminal may draw a frame at every refresh while it scrolls, though its rows change less often: `ttybench scroll` counts the frames that show the text moved.
- **The share of the refreshes of a new frame**, **the frames shown a second** and **the frames dropped** come of the display's own record of what it showed at each refresh: a frame drawn but never shown counts dropped. The share of the refreshes counts against the display's rate where it tells one, else against the time that a surface stays on screen. A display of a variable rate, such as a MacBook Pro's at 120 Hz, may refresh a little more often than its rate says while frames come: the frames shown a second may then pass the rate, and the share stops at 100 %.
- **The trace** takes some tens of megabytes a second: ttybench asks for 2 GiB free first, and removes it once read, whatever comes. `--keep` keeps it for Instruments to open.

Two terminals at 60 Hz, 10 seconds of a trackpad's scroll each, of the same feature:

| Terminal | Scroll | Frames a second | A frame's GPU time, median and 95th percentile | Refreshes of a new frame | Frames dropped |
|---|---|---|---|---|---|
| Lokyr 1.4.0 | smooth | 59.5 | 0.146 · 0.158 ms | 98.8 % | 0 |
| Ghostty 1.4.0 | smooth | skipped: no smooth scrolling | | | |
| Lokyr 1.4.0 | by rows | 9.7 | 0.143 · 0.146 ms | 16.0 % | 0 |
| Ghostty 1.4.0 | by rows | 58.8 | 0.175 · 0.180 ms | 97.4 % | 1 |

The same scroll on a MacBook Pro's built-in display at 120 Hz, of Lokyr's smooth scrolling: 118.2 frames a second, 117.7 of them shown, 98.1 % of the refreshes of a new frame, 2 frames dropped, a frame's GPU time 0.296 · 0.341 ms.

### motion

How smoothly a terminal's window moves: dragged by its titlebar or resized by its corner, the frames that the display showed, those that it missed, the compositor's delay, and the CPU of the terminal and of WindowServer.

```sh
ttybench motion --terminal ghostty --kind drag --seconds 10
ttybench motion --terminal ghostty --kind resize
```

Each round launches the terminal running `ttybench-payload` at rest, records Instruments' Animation Hitches of every process with `xctrace`, while the mouse drags the window by its titlebar or resizes it by its bottom-right corner along a sine of `--amplitude` points, 200 by default, and of 2 seconds, `--rate` events a second, 120 by default, as a hand moves back and forth; then reads the trace, over the motion less its first and its last 0.3 seconds, and removes it.

- **The frames a second** that the display showed, of its surfaces' swaps, and **the frames missed**, of the gaps between two swaps longer than a refresh and a half.
- **The compositor's delay** of each frame, in milliseconds, as the window server tells it.
- **The CPU** of the terminal's processes and of WindowServer, in percent of a core.
- **The trace** of every process takes much room: ttybench asks for 4 GiB free, and removes it once read, unless `--keep`.
- **Leave the mouse and the trackpad alone**: the motion goes through the window server.

Lokyr's window dragged 10 seconds at 60 Hz, measured with ttybench and with the scripts that measured its releases before, of the same build and config:

| Measure | Frames a second | Missed | Compositor's delay, median and 95th percentile | Lokyr's CPU | WindowServer's CPU |
|---|---|---|---|---|---|
| The former scripts | 59.0 | 10 | 12.97 · 14.00 ms | 3.3 % | 33.7 % |
| ttybench | 59.9 | 1 | 12.63 · 14.00 ms | 4.1 % | 35.8 % |

The frames missed vary from a run to the next, from none to ten, for both.

## Limits

- **macOS on Apple Silicon alone**: the window server, ScreenCaptureKit, IOKit's GPU clients and Instruments are macOS's; the processes' GPU time is the Apple GPU's.
- **To the window server, not to the light**: `latency` ends at the frame that the window server took from the terminal, and `scroll` counts the frames that it took. The display's scan-out and its panel's response come after, alike for every terminal on that display; a camera would see them, ttybench doesn't.
- **A frame's time is the window server's**: each capture's frame tells when the window server took it, within a refresh of the key's echo, not when the display showed it. `frames` reads which frames the display showed from its own record, in Instruments' trace.
- **A startup through `open`** counts Launch Services' work and the app's opening of a script, more than a terminal launched by its own program does: compare startups among terminals launched alike.
- **One window**: the tools measure a terminal's first window, and a pane beside it for `latency --flood`; tabs, splits and other windows aren't measured.
- **A virtual machine's GPU** tells no process's GPU time: `gpu` stops there, of its reason, and `frames` reads what Instruments' trace holds of it.
- **The system's GPU** is the whole GPU's: `powermetrics` tells its power and its share of the time at work, not each process's. A terminal's own share is its GPU time.
- **Shortcuts of the US layout**: `beside` sends a shortcut's character with its key, which an app that reads the key's place on another layout may not take; give it a click.
- **Traces take room**: `frames` and `motion` ask for 2 and 4 GiB free, and remove each trace once read.
- **Figures compare within a session**: a history of sessions tells a trend, within a few percent.

## License

MIT, as [LICENSE](LICENSE) says.
