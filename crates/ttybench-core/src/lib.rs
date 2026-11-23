//! The library that ttybench's tools share. Each tool is a binary of its
//! own, `ttybench-<tool>`, usable alone, which the dispatcher `ttybench`
//! runs as `ttybench <tool>`, as `git` runs its commands.

pub mod config;
pub mod keys;
pub mod process;
pub mod terminal;
pub mod tools;
pub mod windows;
