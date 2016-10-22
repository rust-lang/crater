use errors::*;
use std::thread;
use std::time::Duration;
use std::path::Path;
use std::fs;
use std::any::Any;

pub fn try_hard<F, R>(f: F) -> Result<R>
    where F: Fn() -> Result<R>
{
    try_hard_limit(1000, f)
}

pub fn try_hard_limit<F, R>(ms: usize, f: F) -> Result<R>
    where F: Fn() -> Result<R>
{
    let mut r = Err("".into());
    for i in 1..3 {
        r = f();
        if r.is_ok() {
            return r;
        } else if let Err(ref e) = r {
            log_err!("{}", e);
        };
        log!("retrying in {}ms", i * ms);
        thread::sleep(Duration::from_millis((i * ms) as u64));
    }

    f()
}

pub fn remove_dir_all(dir: &Path) -> Result<()> {
    try_hard_limit(10, || {
        fs::remove_dir_all(dir)?;
        if dir.exists() {
            return Err("unable to remove directory".into());
        } else {
            Ok(())
        }
    })
}

pub fn report_error(e: &Error) {
    log_err!("{}", e);

    for e in e.iter().skip(1) {
        log_err!("caused by: {}", e);
    }
}

pub fn report_panic(e: &Any) {
    if let Some(e) = e.downcast_ref::<String>() {
        log_err!("panicked: {}", e);
    } else if let Some(e) = e.downcast_ref::<&'static str>() {
        log_err!("panicked: {}", e);
    } else {
        log_err!("panicked");
    }
}

