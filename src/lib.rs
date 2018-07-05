#![recursion_limit = "128"]
#![deny(unused_extern_crates)]
extern crate base64;
extern crate chrono;
extern crate crates_index;
extern crate crossbeam;
#[macro_use]
extern crate error_chain;
extern crate flate2;
extern crate futures;
extern crate futures_cpupool;
extern crate handlebars;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate libc;
extern crate mime;
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
extern crate rustup_dist;
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
    slog_o, slog_info, slog_log, slog_error, slog_warn, slog_debug, slog_record, slog_record_static,
    slog_b, slog_kv
)]
extern crate slog;
#[macro_use]
extern crate slog_scope;
extern crate slog_term;
extern crate tar;
extern crate tempdir;
extern crate tempfile;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_process;
extern crate tokio_timer;
extern crate toml;
#[macro_use]
extern crate url;
extern crate walkdir;

pub mod agent;
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
