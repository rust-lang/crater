#![recursion_limit = "256"]
#![allow(clippy::needless_pass_by_value, clippy::new_ret_no_self)]

#[cfg_attr(test, macro_use)]
extern crate toml;

pub mod actions;
pub mod agent;
mod assets;
pub mod logs;
#[macro_use]
pub mod utils;
pub mod config;
pub mod crates;
pub mod db;
pub mod dirs;
mod docker;
pub mod experiments;
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
