use crate::prelude::*;
use failure::AsFail;
use std::any::Any;
use std::fmt::Display;
use std::thread;
use std::time::Duration;

pub(crate) mod fs;
pub(crate) mod hex;
pub(crate) mod http;
#[macro_use]
mod macros;
pub mod size;
pub(crate) mod string;

pub(crate) fn try_hard<F, R, E>(f: F) -> Result<R, E>
where
    F: Fn() -> Result<R, E>,
    E: Display,
{
    try_hard_limit(1000, f)
}

pub(crate) fn try_hard_limit<F, R, E>(ms: usize, f: F) -> Result<R, E>
where
    F: Fn() -> Result<R, E>,
    E: Display,
{
    let mut r;
    for i in 1..3 {
        r = f();
        if r.is_ok() {
            return r;
        } else if let Err(ref e) = r {
            error!("{}", e);
        };
        info!("retrying in {}ms", i * ms);
        thread::sleep(Duration::from_millis((i * ms) as u64));
    }

    f()
}

pub fn report_panic(e: &Any) {
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
