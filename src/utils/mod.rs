use crate::prelude::*;
use failure::AsFail;
use percent_encoding::{AsciiSet, CONTROLS};
use std::any::Any;

pub(crate) mod hex;
pub(crate) mod http;
#[macro_use]
mod macros;
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

pub fn report_failure<F: AsFail>(err: &F) {
    let err = err.as_fail();
    error!("{}", err);

    for cause in err.iter_causes() {
        error!("caused by: {}", cause);
    }

    if let Some(backtrace) = err.backtrace() {
        error!("{}", backtrace);
    }
}
