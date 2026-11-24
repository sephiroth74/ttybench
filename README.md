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
- **A run that fails** is told, with its output's last lines, and the session goes on. `ttybench run --resume <folder>` takes up a session that was cut off, or runs its failed runs again; what ended well stays as it is.
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
- **A person at the Mac**: macOS counts ttybench's keys, clicks and scrolls as no one's activity, and would let the display sleep in the middle of a session, which leaves nothing to capture. ttybench tells macOS that someone is at the Mac every second while it measures, as a person's typing does; `--presence off` doesn't.
- **Your Mac's state counts**: the same terminal can show a latency some milliseconds lower while you work on the Mac than while it rests. Measure every terminal of a comparison in the same state, and in one session.
- **What the capture takes**: `--source window`, the default, takes the window alone, each frame as the window server takes the window's new content. `--source screen` takes the screen where the window lies, each frame as the window server composes it, a few milliseconds later, with whatever lies over the window. While ttybench captures a window, macOS shows its capture indicator in the place of the window's buttons, away from the typed row.
- **Permissions**: the capture takes the Screen Recording permission, and the keys the Accessibility permission, of the app that runs ttybench, such as your terminal or your editor.

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

## License

MIT, as [LICENSE](LICENSE) says.
