#![warn(missing_docs)]
#![allow(clippy::new_without_default)]

//! rustwide is a library to execute your code against the Rust ecosystem, powering projects like
//! [Crater][crater].
//!
//! [crater]: https://github.com/rust-lang/crater

pub mod cmd;
pub mod logging;
mod native;
mod utils;
mod workspace;

pub use crate::workspace::{Workspace, WorkspaceBuilder};
