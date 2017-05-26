#![recursion_limit = "1024"]

#![allow(unused_must_use)]

extern crate rand;
extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate tempdir;
extern crate url;
extern crate walkdir;
extern crate semver;
#[macro_use]
extern crate lazy_static;
extern crate chrono;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde;
extern crate tar;
extern crate flate2;
extern crate toml;
#[macro_use]
extern crate scopeguard;
extern crate libc;
extern crate winapi;
extern crate kernel32;
extern crate reqwest;
#[macro_use(slog_o, slog_info, slog_log, slog_error,
            slog_record, slog_record_static, slog_b, slog_kv)]
extern crate slog;
#[macro_use]
extern crate slog_scope;
extern crate slog_term;
extern crate futures;
extern crate tokio_core;
extern crate tokio_process;
extern crate tokio_io;
extern crate tokio_timer;
extern crate result;
extern crate ref_slice;
extern crate crates_index;

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
