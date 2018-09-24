use errors::*;
use std::any::Any;
use std::thread;
use std::time::Duration;

pub(crate) mod fs;
pub(crate) mod hex;
pub(crate) mod http;
#[macro_use]
mod macros;
pub mod size;
pub(crate) mod string;

pub fn try_hard<F, R>(f: F) -> Result<R>
where
    F: Fn() -> Result<R>,
{
    try_hard_limit(1000, f)
}

pub fn try_hard_limit<F, R>(ms: usize, f: F) -> Result<R>
where
    F: Fn() -> Result<R>,
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

pub fn report_error(e: &Error) {
    error!("{}", e);

    for e in e.iter().skip(1) {
        error!("caused by: {}", e)
    }

    if let Some(backtrace) = e.backtrace() {
        error!("{:?}", backtrace);
    }
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
