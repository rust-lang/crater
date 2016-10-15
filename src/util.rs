use errors::*;
use std::thread;
use std::time::Duration;

pub fn try_hard<F, R>(f: F) -> Result<R>
    where F: Fn() -> Result<R>
{
    let mut r = Err("".into());
    for i in 1 .. 4 {
        let r = f();
        if r.is_ok() {
            return r;
        }

        log!("op failed. retrying in {}s", i);
        thread::sleep(Duration::from_secs(i));
    }

    r
}
