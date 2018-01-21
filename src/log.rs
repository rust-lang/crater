use chrono::Utc;
use dirs::LOG_DIR;
use errors::*;
use slog::{self, Drain};
use slog_scope;
use slog_term;
use std::env;
use std::fs;
use std::fs::{File, OpenOptions};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn global_log_name() -> &'static Path {
    lazy_static! {
        static ref PATH: PathBuf = LOG_DIR.join(
                format!("{}", Utc::now().format("%Y-%m-%dT%H-%M-%S.%f")));
    };
    &*PATH
}

pub fn redirect<F, R>(path: &Path, f: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    let file = file_drain(path);
    let term = TERM_DRAIN.clone();

    let drain = slog::Duplicate(term, file).fuse();
    slog_scope::scope(&slog::Logger::root(drain, slog_o!()), f)
}

lazy_static! {
    static ref START_TIME: Instant = Instant::now();
}

lazy_static! {
    static ref TERM_DRAIN: Arc<slog::Fuse<Mutex<
        slog_term::CompactFormat<slog_term::TermDecorator>>>> = {
        let plain = slog_term::TermDecorator::new().stdout().build();
        let term = Mutex::new(slog_term::CompactFormat::new(plain).build()).fuse();
        Arc::new(term)
    };
}

fn file_drain(
    path: &Path,
) -> slog::Fuse<slog_term::FullFormat<slog_term::PlainSyncDecorator<File>>> {
    let f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("Could not open log file.");
    let decorator = slog_term::PlainSyncDecorator::new(f);
    slog_term::FullFormat::new(decorator).build().fuse()
}

pub fn init() -> slog_scope::GlobalLoggerGuard {
    START_TIME.deref();

    fs::create_dir_all(&*LOG_DIR).expect("Could create log directory.");
    let file = file_drain(global_log_name());
    let term = TERM_DRAIN.clone();

    let drain = slog::Duplicate(term, file).fuse();
    let _guard = slog_scope::set_global_logger(slog::Logger::root(drain, slog_o!{}));

    info!(
        "program args: {}",
        env::args().skip(1).collect::<Vec<_>>().join(" ")
    );

    _guard
}

pub fn finish() {
    let duration = Instant::now().duration_since(*START_TIME).as_secs();
    let duration = if duration < 60 {
        format!("{}s", duration)
    } else {
        let minutes = duration / 60;
        let seconds = duration % 60;
        format!("{}m {}s", minutes, seconds)
    };
    info!("logs: {}", global_log_name().display());
    info!("duration: {}", duration);
}
