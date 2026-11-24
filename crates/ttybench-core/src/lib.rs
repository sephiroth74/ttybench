//! The library that ttybench's tools share. Each tool is a binary of its
//! own, `ttybench-<tool>`, usable alone, which the dispatcher `ttybench`
//! runs as `ttybench <tool>`, as `git` runs its commands.

pub mod awake;
pub mod budget;
pub mod capture;
pub mod cli;
pub mod clock;
pub mod config;
pub mod frames;
pub mod gpu;
pub mod keys;
pub mod payload;
pub mod process;
pub mod results;
pub mod scroll;
pub mod stats;
pub mod terminal;
pub mod tools;
pub mod windows;
pub mod xctrace;
