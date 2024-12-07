use crate::prelude::*;
use failure::{AsFail, Backtrace};
use percent_encoding::{AsciiSet, CONTROLS};
use std::any::Any;
use std::fmt;
use std::str::FromStr;

pub(crate) mod hex;
pub(crate) mod http;
#[macro_use]
mod macros;
pub(crate) mod disk_usage;
pub(crate) mod path;
pub(crate) mod serialize;
pub mod size;
pub(crate) mod string;

/// The set of characters which cannot be used in a [filename on Windows][windows].
///
/// [windows]: https://docs.microsoft.com/en-us/windows/desktop/fileio/naming-a-file#naming-conventions
pub(crate) const FILENAME_ENCODE_SET: AsciiSet = CONTROLS
    .add(b'<')
    .add(b'>')
    .add(b':')
    .add(b'"')
    .add(b'/')
    .add(b'\\')
    .add(b'|')
    .add(b'?')
    .add(b'*');

pub fn report_panic(e: &dyn Any) {
    if let Some(e) = e.downcast_ref::<String>() {
        error!("panicked: {}", e)
    } else if let Some(e) = e.downcast_ref::<&'static str>() {
        error!("panicked: {}", e)
    } else {
        error!("panicked")
    }
}

pub fn report_failure(err: &(impl HasBacktrace + AsFail)) {
    let backtrace = err.backtrace();
    let err = err.as_fail();
    error!("{}", err);

    for cause in err.iter_causes() {
        error!("caused by: {}", cause);
    }

    let backtrace = match backtrace {
        Some(bt) => bt,
        None => {
            error!("no backtrace");
            return;
        }
    };

    // Avoid printing a blank line if the backtrace exists but is empty.
    //
    // This can occur if backtraces are not enabled on this platform or if the requisite
    // environment variables are not set.
    let backtrace = backtrace.to_string();
    if !backtrace.is_empty() {
        error!("{}", backtrace);
        return;
    }

    // If the the environment variable is not set, mention it to the user.
    if !is_backtrace_runtime_enabled() {
        error!("note: run with `RUST_BACKTRACE=1` to display a backtrace.");
    }
}

pub trait HasBacktrace {
    fn backtrace(&self) -> Option<&Backtrace>;
}

impl HasBacktrace for failure::Error {
    fn backtrace(&self) -> Option<&Backtrace> {
        Some(Self::backtrace(self))
    }
}

impl<T> HasBacktrace for failure::Context<T>
where
    T: fmt::Display + Send + Sync + 'static,
{
    fn backtrace(&self) -> Option<&Backtrace> {
        Fail::backtrace(self)
    }
}

fn is_backtrace_runtime_enabled() -> bool {
    std::env::var("RUST_BACKTRACE")
        .ok()
        .and_then(|s| i32::from_str(&s).ok())
        .is_some_and(|val| val != 0)
}
