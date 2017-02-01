use errors::*;
use std::thread;
use std::time::Duration;
use std::path::{Path, PathBuf};
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
            bail!("unable to remove directory");
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

pub fn this_target() -> String {
    let os = if cfg!(target_os = "linux") {
        "unknown-linux-gnu"
    } else if cfg!(target_os = "windows") {
        "pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        panic!("unrecognized OS");
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        panic!("unrecognized arch");
    };

    format!("{}-{}", arch, os)
}

pub fn copy_dir(src_dir: &Path, dest_dir: &Path) -> Result<()> {
    use walkdir::*;

    log!("copying {} to {}", src_dir.display(), dest_dir.display());

    if dest_dir.exists() {
        remove_dir_all(dest_dir)
            .chain_err(|| "unable to remove test dir")?;
    }
    fs::create_dir_all(dest_dir)
        .chain_err(|| "unable to create test dir")?;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry.file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
    }

    let mut partial_dest_dir = PathBuf::from("./");
    let mut depth = 0;
    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry.chain_err(|| "walk dir")?;
        while entry.depth() <= depth && depth > 0 {
            assert!(partial_dest_dir.pop());
            depth -= 1;
        }
        let path = dest_dir.join(&partial_dest_dir).join(entry.file_name());
        if entry.file_type().is_dir() && entry.depth() > 0 {
            fs::create_dir_all(&path)?;
            assert!(entry.depth() == depth + 1);
            partial_dest_dir.push(entry.file_name());
            depth += 1;
        }
        if entry.file_type().is_file() {
            fs::copy(&entry.path(), path)?;
        }
    }

    Ok(())
}
