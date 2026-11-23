//! The library that ttybench's tools share. Each tool is a binary of its
//! own, `ttybench-<tool>`, usable alone, which the dispatcher `ttybench`
//! runs as `ttybench <tool>`, as `git` runs its commands.

pub mod config;
pub mod tools;
