#![recursion_limit = "128"]
#![deny(unused_extern_crates)]
#![cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]

extern crate base64;
extern crate bytes;
extern crate chrono;
extern crate chrono_humanize;
extern crate crates_index;
extern crate crossbeam_utils;
#[macro_use]
extern crate error_chain;
extern crate flate2;
extern crate futures;
extern crate futures_cpupool;
extern crate http;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
#[cfg(not(windows))]
extern crate libc;
extern crate mime;
extern crate minifier;
extern crate petgraph;
extern crate r2d2;
extern crate r2d2_sqlite;
extern crate rand;
extern crate ref_slice;
extern crate regex;
extern crate reqwest;
extern crate ring;
extern crate rusoto_core;
extern crate rusoto_s3;
extern crate rusqlite;
#[macro_use]
extern crate scopeguard;
extern crate semver;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate serde_regex;
#[macro_use(
    slog_o,
    slog_info,
    slog_log,
    slog_error,
    slog_warn,
    slog_record,
    slog_record_static,
    slog_b,
    slog_kv
)]
extern crate slog;
#[macro_use]
extern crate slog_scope;
extern crate slog_term;
extern crate tar;
extern crate tempdir;
extern crate tempfile;
extern crate tera;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_process;
extern crate tokio_timer;
#[cfg_attr(test, macro_use)]
extern crate toml;
#[macro_use]
extern crate url;
extern crate walkdir;
extern crate warp;
#[cfg(windows)]
extern crate winapi;

pub mod agent;
mod assets;
#[macro_use]
pub mod log;
#[macro_use]
pub mod util;
pub mod config;
pub mod crates;
pub mod dirs;
mod dl;
pub mod docker;
pub mod errors;
pub mod ex;
pub mod ex_run;
mod file;
mod gh;
mod git;
pub mod lists;
mod registry;
pub mod report;
pub mod results;
mod run;
pub mod run_graph;
pub mod server;
mod tasks;
mod toml_frobber;
pub mod toolchain;

pub static GIT_REVISION: Option<&'static str> = include!(concat!(env!("OUT_DIR"), "/sha"));
