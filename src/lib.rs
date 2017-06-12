#![deny(unused_extern_crates)]
extern crate arc_cell;
extern crate chrono;
extern crate crates_index;
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
extern crate rand;
extern crate ref_slice;
extern crate reqwest;
extern crate route_recognizer;
extern crate rusoto_core;
extern crate rusoto_s3;
extern crate rustup_dist;
#[macro_use]
extern crate scopeguard;
extern crate semver;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use(slog_o, slog_info, slog_log, slog_error, slog_debug, slog_record, slog_record_static,
            slog_b, slog_kv)]
extern crate slog;
#[macro_use]
extern crate slog_scope;
extern crate slog_term;
extern crate tar;
extern crate tempdir;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_process;
extern crate tokio_timer;
extern crate toml;
extern crate uri;
extern crate url;
extern crate walkdir;

#[macro_use]
pub mod log;
pub mod errors;
pub mod toolchain;
mod registry;
pub mod lists;
mod file;
mod dl;
mod gh;
pub mod util;
mod run;
mod crates;
mod git;
pub mod ex;
pub mod ex_run;
mod toml_frobber;
mod gh_mirrors;
pub mod report;
pub mod docker;
pub mod dirs;
mod results;
pub mod server;
