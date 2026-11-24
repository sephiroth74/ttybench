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

- `launch` is the program and its arguments, with `{app}` for the bundle, `{command}` for the script that ttybench runs in the terminal, and `{cwd}` for the folder it starts in. A terminal that takes no command on its command line, such as Terminal.app, iTerm2 or Warp, uses `"open"`: ttybench opens a `.command` script with the app.
- `env` gives the terminal more environment variables, such as the folder of a config made for the benchmarks.
- Give every terminal the same font, font size and window size, so that their figures compare.

The tools read `--terminals <file>`, else `$TTYBENCH_TERMINALS`, else `terminals.toml` in the current folder, else `~/.config/ttybench/terminals.toml`.

### Measure the same thing of every terminal

A measure compares terminals only when each runs of the same conditions: give every terminal a base config alike, of the same font, font size and window size, a cursor that doesn't blink, no ligatures, an opaque window, no plugins or extensions of its own, and every feature that another terminal may lack turned off. ttybench can't read two terminals' configs and tell them alike: that is yours to do, in the configs that your terminals' file points them at. Each record keeps the feature that it measured, if one, so that reports compare records of the same conditions alone.

### Features

Some measures compare a feature that not every terminal has, such as smooth scrolling. Describe each terminal's features and how each turns on, and give it a base config of every feature off:

```toml
[terminals.lokyr]
app = "/Applications/Lokyr.app"
launch = "open"
env = { XDG_CONFIG_HOME = "~/ttybench/lokyr" }                       # smooth scrolling off
features.smooth-scroll = { env = { XDG_CONFIG_HOME = "~/ttybench/lokyr-smooth" } }

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

- **Balanced orders**: each cycle turns each run's terminals by one, so that over as many cycles as a run has terminals each one runs first as often as last. ttybench tells you when the cycles don't balance them.
- **The session's folder**, `sessions/<date>` by default, keeps a copy of the session's and the terminals' files, `machine.json`, `runs.jsonl` of each run's end, `records.jsonl` of every record, each of its cycle, and a folder of each run of its output.
- **A run that fails** is told, with its output's last lines, and the session goes on. **A run skipped**, of a feature that its terminal doesn't have, is told apart, and `--resume` doesn't run it again. `ttybench run --resume <folder>` takes up a session that was cut off, or runs its failed runs again; what ended well stays as it is.
- **`feature`** on a run measures the terminals that have that feature, of it on, and skips the others, without launching them: `--dry-run` tells them, and the session's log keeps them as skipped. A run of no `feature` measures each terminal's base.
- **`season`** launches each terminal that many times before the first run, not counted, as a terminal just built or installed starts slower for its first launches.
- **`sudo = true`** on a run that needs `sudo`: the session asks for it once and keeps it.

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

Each round launches the terminal running `ttybench-payload`, which prints a file of `--mib` MiB, 256 by default, as fast as the terminal takes it: a smaller file leaves more of its time to the window's first frames. Two rates come of each round: to the last byte's write, as the terminal took the bytes, and to the quiet, as its processes' CPU drops below 5 % of a core for 60 ms, once it has drawn what it took. `--text` is `plain`, `colors` or `wide`.

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
- **Leave the mouse and the trackpad alone**: the scroll goes through the window server to the window under the pointer, which ttybench moves to the window's middle.
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
- **The share of the refreshes of a new frame** and **the frames dropped** come of the display's own record of what it showed at each refresh: a frame drawn but never shown counts dropped. The share of the refreshes counts against the display's rate where it tells one, else against the time that a surface stays on screen.
- **The trace** takes some tens of megabytes a second: ttybench asks for 2 GiB free first, and removes it once read, whatever comes. `--keep` keeps it for Instruments to open.

Two terminals at 60 Hz, 10 seconds of a trackpad's scroll each, of the same feature:

| Terminal | Scroll | Frames a second | A frame's GPU time, median and 95th percentile | Refreshes of a new frame | Frames dropped |
|---|---|---|---|---|---|
| Lokyr 1.4.0 | smooth | 59.5 | 0.146 · 0.158 ms | 98.8 % | 0 |
| Ghostty 1.4.0 | smooth | skipped: no smooth scrolling | | | |
| Lokyr 1.4.0 | by rows | 9.7 | 0.143 · 0.146 ms | 16.0 % | 0 |
| Ghostty 1.4.0 | by rows | 58.8 | 0.175 · 0.180 ms | 97.4 % | 1 |

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

## License

MIT, as [LICENSE](LICENSE) says.
