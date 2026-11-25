# Changelog

Every change that you can notice, newest first, of the format of [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org).

## [0.2.0] - 2026-11-25

### Added

- ttybench on crates.io: `cargo install ttybench` installs the dispatcher and every tool side by side.
- `startup --floor`: the least app that shows a window, `ttybench-floor`, launched beside each round as the terminal launches, and the terminal's window less the floor's of the same round, `window_gap_ms`, out of which what slows both alike drops, as the Mac's state from a day to the next.
- A style file, `style.toml`, of a session or of `--style`: which charts are drawn and of which metrics, their width, font and colors, each terminal's own color, the report's columns, their titles and the metrics' units, and each metric's statistic of the A/B.

### Changed

- An A/B compares the wakeups at rest of their means, whose medians are often 0 of both copies.
- One package, `ttybench`, of a library and every tool's binary, in place of fifteen; the binaries' names and their command lines stay as they were.
- It builds with Rust 1.88 or later, and refuses at its build another system than macOS.

## [0.1.0] - 2026-11-24

### Added

- Terminals described in a TOML file of your own: how each launches with a command, its environment, its quit, the keys of a pane beside it, and its features.
- `startup`, `idle`, `memory` and `throughput`: the time to a window and to a command, the CPU, wakeups and power at rest, the memory's footprint, and the rate of printing plain, colored and wide text.
- `latency`: a key's press to the frame that shows it, through ScreenCaptureKit, at every phase of the refresh or at one, and beside a pane that floods.
- `scroll`: the frames that show the text moved, of a trackpad's scroll or a wheel's.
- `gpu`: the terminal's and WindowServer's GPU time, and the system's GPU of `powermetrics`.
- `frames` and `motion`: each frame's GPU time and the display's record of what it showed, and a window dragged or resized, of Instruments' traces.
- `run`: a session of terminals and tools in balanced orders, seasoned, resumable, a feature's runs skipped where a terminal lacks it.
- `report`: each metric's tables of medians and bootstrap intervals, the A/B of two copies of a terminal, and budgets.
- `charts`: SVG charts of each metric, the latency's distribution, the frames in time, the GPU and the A/B, a static page of the report and its charts, and the history of a terminal along its versions.

[0.2.0]: https://github.com/sephiroth74/ttybench/releases/tag/v0.2.0
[0.1.0]: https://github.com/sephiroth74/ttybench/releases/tag/v0.1.0
