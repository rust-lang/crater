use std::time::Instant;
use std::mem;
use std::env;
use std::io::{self, Write, Read, BufReader, BufRead};
use file;
use std::path::{PathBuf, Path};
use std::time::SystemTime;
use chrono::UTC;
use LOG_DIR;
use std::fs;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::ops::Deref;
use errors::*;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel, Sender};
use std::cell::RefCell;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
use kernel32;
use winapi;

lazy_static! {
    static ref LOCK: Mutex<()> = Mutex::new(());
}

fn log(line: &str) {
    let _g = LOCK.lock();
    println!("{}", line);
    log_to_file(&out_file(), line);
}

fn log_err(line: &str) {
    let _g = LOCK.lock();
    writeln!(&mut io::stderr(), "{}", line);
    log_to_file(&out_file(), line);
}

fn log_to_file(file: &Path, line: &str) {
    fs::create_dir_all(LOG_DIR);
    file::append_line(file, line).expect(&format!("unable to write log to {}", file.display()));
}

fn out_file() -> PathBuf {
    if let Some(r) = redirected_file() {
        r
    } else {
        PathBuf::from(format!("{}/{}.txt", LOG_DIR, global_log_name()))
    }
}

fn global_log_name() -> &'static str {
    lazy_static! {
        static ref NAME: String = format!("{}", UTC::now().format("%Y-%m-%dT%H-%M-%S.%f"));
    }
    &*NAME
}

pub fn redirect<F, R>(path: &Path, f: F) -> Result<R>
    where F: FnOnce() -> Result<R>
{
    log_local_stdout(&format!("logging to {}", path.display()));
    let mut old = swap_redirect(path.to_owned());
    defer!{{
        let old = old.take();
        old.and_then(swap_redirect);
    }}
    f()
}

lazy_static! {
    static ref REDIRECT_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
}

fn swap_redirect(path: PathBuf) -> Option<PathBuf> {
    let mut redirect = REDIRECT_FILE.lock().expect("");
    let redirect: &mut Option<PathBuf> = &mut *redirect;
    mem::replace(redirect, Some(path))
}

fn redirected_file() -> Option<PathBuf> {
    let mut redirect = REDIRECT_FILE.lock().expect("");
    let redirect: &mut Option<PathBuf> = &mut *redirect;
    redirect.clone()
}

macro_rules! log {
    ($fmt:expr) => {
        $crate::log::log_local_stdout(&format!($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::log::log_local_stdout(&format!($fmt, $($arg)*))
    };
}

macro_rules! log_err {
    ($fmt:expr) => {
        $crate::log::log_local_stderr(&format!($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::log::log_local_stderr(&format!($fmt, $($arg)*))
    };
}

pub fn log_local_stdout(line: &str) {
    log(&format!("boom! {}", line));
}

pub fn log_local_stderr(line: &str) {
    log(&format!("kaboom! {}", line));
}

pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

pub fn log_command(mut cmd: Command) -> Result<ProcessOutput> {
    log_command_(cmd, false)
}

pub fn log_command_capture(mut cmd: Command) -> Result<ProcessOutput> {
    log_command_(cmd, true)
}

const MAX_TIMEOUT_SECS: u64 = 60 * 10 * 2;
const HEARTBEAT_TIMEOUT_SECS: u64 = 60 * 2;

pub fn log_command_(mut cmd: Command, capture: bool) -> Result<ProcessOutput> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("");
    let stderr = child.stderr.take().expect("");

    // Needed for killing after timeout
    let child_id = child.id();

    // Child's stdio needs to produce output to avoid being killed
    let (heartbeat_tx, heartbeat_rx) = mpsc::channel();

    let rx_out = sink(Box::new(stdout), log_child_stdout, capture, heartbeat_tx.clone());
    let rx_err = sink(Box::new(stderr), log_child_stderr, capture, heartbeat_tx);

    #[cfg(unix)]
    fn kill_process(id: u32) {
        use libc::{kill, SIGKILL, pid_t};
        let r = unsafe { kill(id as pid_t, SIGKILL) };
        if r != 0 {
            // Something went wrong...
        }
    }
    #[cfg(windows)]
    fn kill_process(id: u32) {
        unsafe {
            let handle = kernel32::OpenProcess(winapi::winnt::PROCESS_TERMINATE, 0, id);
            kernel32::TerminateProcess(handle, 101);
            if kernel32::CloseHandle(handle) == 0 {
                panic!("CloseHandle for process {} failed", id);
            }
        };
    }

    // Have another thread kill the subprocess after the maximum timeout
    let (timeout_tx, timeout_rx) = mpsc::channel();
    let (timeout_cancel_tx, timeout_cancel_rx) = mpsc::channel();
    thread::spawn(move || {
        let timeout = Duration::from_secs(MAX_TIMEOUT_SECS);
        match timeout_cancel_rx.recv_timeout(timeout) {
            Ok(_) => {
                // Child process exited
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                kill_process(child_id);
                timeout_tx.send(());
            }
            _ => { panic!() }
        }
    });

    // Have another thread listening for heartbeats on stdout/stderr
    let (heartbeat_timeout_tx, heartbeat_timeout_rx) = mpsc::channel();
    let (heartbeat_cancel_tx, heartbeat_cancel_rx) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let timeout = Duration::from_secs(HEARTBEAT_TIMEOUT_SECS);
            match heartbeat_cancel_rx.recv_timeout(timeout) {
                Ok(_) => {
                    // Child process exited
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    let heartbeats = heartbeat_rx.try_iter().count();
                    if heartbeats > 0 {
                        continue;
                    } else {
                        // No heartbeats before timeout
                        kill_process(child_id);
                        heartbeat_timeout_tx.send(());
                    }
                }
                _ => { panic!() }
            }
        }
    });

    let status = child.wait();
    timeout_cancel_tx.send(());
    heartbeat_cancel_tx.send(());
    let timed_out = timeout_rx.try_recv().is_ok();
    let heartbeat_timed_out = heartbeat_timeout_rx.try_recv().is_ok();
    let status = status?;
    let stdout = rx_out.recv().expect("");
    let stderr = rx_err.recv().expect("");

    if heartbeat_timed_out {
        log!("process killed after not generating output for {} s", HEARTBEAT_TIMEOUT_SECS);
        bail!(ErrorKind::Timeout);
    } else if timed_out {
        log!("process killed after max time of {} s", MAX_TIMEOUT_SECS);
        bail!(ErrorKind::Timeout);
    }

    Ok(ProcessOutput {
        status: status,
        stdout: stdout,
        stderr: stderr,
    })
}

pub fn log_child_stdout(line: &str) {
    log(&format!("blam! {}", line));
}

pub fn log_child_stderr(line: &str) {
    log(&format!("kablam! {}", line));
}

fn sink(reader: Box<Read + Send>, log: fn (&str),
        capture: bool, heartbeat_tx: Sender<()>) -> Receiver<Vec<String>> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let reader = BufReader::new(reader);
        for line_bytes in reader.split(b'\n') {
            if let Ok(mut line_bytes) = line_bytes {
                let line = String::from_utf8_lossy(&line_bytes);
                log(line.deref());
                heartbeat_tx.send(());
                if capture {
                    buf.push(line.to_string());
                }
            } else {
                log("READING FROM CHILD PROCESS FAILED!");
            }
        }

        tx.send(buf).expect("");
    });

    rx
}

lazy_static! {
    static ref START_TIME: Instant = Instant::now();
}

pub fn init() {
    START_TIME.deref();
    log!("program args: {}", env::args().skip(1).collect::<Vec<_>>().join(" "));
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
    log!("logs: {}", out_file().display());
    log!("duration: {}", duration);
}
