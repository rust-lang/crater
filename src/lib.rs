//! Crater is a tool for testing Rust compiler changes against the crate ecosystem in crates.io.
//!
//! It compares two toolchains by building a set of crates against both and reporting
//! regressions. This is primarily used by the Rust project to evaluate the impact of
//! compiler and standard library changes before they land.
//!
//! # Modules
//!
//! - [`actions`] — Command pattern for mutating experiment state
//! - [`agent`] — Distributed worker agents that poll the server for work
//! - [`config`] — Configuration loading from `config.toml`
//! - [`crates`] — Crate abstraction (registry, GitHub, local, git)
//! - [`db`] — SQLite database layer for persistent state
//! - [`dirs`] — Filesystem layout for the crater work directory
//! - [`experiments`] — Core domain types: `Experiment`, `Status`, `Mode`, etc.
//! - [`report`] — HTML/markdown report generation
//! - [`results`] — Traits and types for per-crate test results and build logs
//! - [`runner`] — Experiment execution engine
//! - [`server`] — HTTP server for the web UI and agent API
//! - [`toolchain`] — Rust toolchain abstraction
//! - [`utils`] — Shared utilities (HTTP, size formatting, hex encoding, etc.)
//!
//! See also the CLI entry point in `src/cli.rs` for the command-line interface, and
//! the [GitHub repository](https://github.com/rust-lang/crater) for architecture notes.

#![recursion_limit = "256"]
#![allow(
    clippy::needless_pass_by_value,
    clippy::wrong_self_convention,
    clippy::new_ret_no_self,
    clippy::too_many_arguments,
    clippy::redundant_closure,
    clippy::unnecessary_wraps,
    clippy::needless_question_mark,
    clippy::vec_init_then_push,
    clippy::upper_case_acronyms,
    clippy::mutex_atomic
)]

pub mod actions;
pub mod agent;
mod assets;
#[macro_use]
pub mod utils;
pub mod config;
pub mod crates;
pub mod db;
pub mod dirs;
pub mod experiments;
mod prelude;
pub mod report;
pub mod results;
pub mod runner;
pub mod server;
pub mod toolchain;

pub(crate) static GIT_REVISION: Option<&str> = include!(concat!(env!("OUT_DIR"), "/sha"));
pub(crate) static CRATER_REPO_URL: &str = "https://github.com/rust-lang/crater";

lazy_static::lazy_static! {
    pub static ref USER_AGENT: String = format!(
        "crater/{} ({})",
        crate::GIT_REVISION.unwrap_or("unknown"),
        crate::CRATER_REPO_URL
    );
}
