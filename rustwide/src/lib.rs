#![warn(missing_docs)]
#![allow(clippy::new_without_default)]

//! rustwide is a library to execute your code against the Rust ecosystem, powering projects like
//! [Crater][crater].
//!
//! [crater]: https://github.com/rust-lang/crater

pub mod cmd;
mod crates;
pub mod logging;
mod native;
mod toolchain;
mod tools;
mod utils;
mod workspace;

pub use crate::crates::Crate;
pub use crate::toolchain::Toolchain;
pub use crate::workspace::{Workspace, WorkspaceBuilder};

pub(crate) static HOST_TARGET: &str = include_str!(concat!(env!("OUT_DIR"), "/target"));
