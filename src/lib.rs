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
