#![recursion_limit = "256"]
#![deny(unused_extern_crates)]
#![cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]

extern crate base64;
extern crate bytes;
extern crate chrono;
extern crate chrono_humanize;
extern crate crates_index;
extern crate crossbeam_utils;
extern crate csv;
#[macro_use]
extern crate failure;
extern crate flate2;
extern crate futures;
extern crate http;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate mime;
extern crate minifier;
#[cfg(unix)]
extern crate nix;
#[macro_use]
extern crate paste;
extern crate petgraph;
extern crate r2d2;
extern crate r2d2_sqlite;
extern crate rand;
extern crate regex;
extern crate reqwest;
extern crate ring;
extern crate rusoto_core;
extern crate rusoto_credential;
extern crate rusoto_s3;
extern crate rusqlite;
#[macro_use]
extern crate scopeguard;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate serde_regex;
#[macro_use(slog_o, slog_info, slog_error, slog_warn,)]
extern crate slog;
#[macro_use]
extern crate slog_scope;
extern crate slog_term;
extern crate tar;
extern crate tempfile;
extern crate tera;
extern crate tokio;
extern crate tokio_process;
#[cfg_attr(test, macro_use)]
extern crate toml;
#[macro_use]
extern crate url;
extern crate walkdir;
extern crate warp;
#[cfg(windows)]
extern crate winapi;

pub mod actions;
pub mod agent;
mod assets;
#[macro_use]
pub mod log;
#[macro_use]
pub mod utils;
pub mod config;
pub mod crates;
pub mod db;
pub mod dirs;
mod docker;
pub mod experiments;
mod git;
mod native;
mod prelude;
pub mod report;
pub mod results;
mod run;
pub mod runner;
pub mod server;
pub mod toolchain;
mod tools;

pub(crate) static GIT_REVISION: Option<&str> = include!(concat!(env!("OUT_DIR"), "/sha"));
pub(crate) static HOST_TARGET: &str = include_str!(concat!(env!("OUT_DIR"), "/target"));
pub(crate) static CRATER_REPO_URL: &str = "https://github.com/rust-lang-nursery/crater";
