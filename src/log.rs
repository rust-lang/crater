use std::io::{self, Write};
use file;
use std::path::{PathBuf, Path};
use std::time::SystemTime;
use chrono::UTC;
use LOG_DIR;
use std::fs;

macro_rules! log {
    ($fmt:expr) => {
        $crate::log::log(concat!("boom! ", $fmt, "\n"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::log::log(&format!(concat!("boom! ", $fmt, "\n"), $($arg)*))
    };
}

macro_rules! log {
    ($fmt:expr) => {
        $crate::log::log(concat!("boom! ", $fmt, "\n"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::log::log(&format!(concat!("boom! ", $fmt, "\n"), $($arg)*))
    };
}

pub fn log(info: &str) {
    print!("{}", info);
    log_to_file(&stdout_file(), info);
}

pub fn log_err(info: &str) {
    write!(&mut io::stderr(), "{}", info);
    log_to_file(&stderr_file(), info);
}

fn log_to_file(file: &Path, info: &str) {
    fs::create_dir_all(LOG_DIR);
    file::append(file, info).expect(&format!("unable to write log to {}", file.display()));
}

fn stdout_file() -> PathBuf {
    PathBuf::from(format!("{}/{}-{}.txt", LOG_DIR, log_name(), "out"))
}

fn stderr_file() -> PathBuf {
    PathBuf::from(format!("{}/{}-{}.txt", LOG_DIR, log_name(), "err"))
}

fn log_name() -> &'static str {
    lazy_static! {
        static ref ROOT: String = format!("{}", UTC::now().format("%Y-%m-%dT%H-%M-%S.%f"));
    }
    &*ROOT
}

pub fn finish() {
    log!("stdout logs: {}", stdout_file().display());
    log!("stderr logs: {}", stderr_file().display());
}
