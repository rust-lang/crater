use errors::*;
use slog::Logger;
use slog_scope;
use std::io::{BufRead, BufReader, Read};
use std::ops::Deref;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

pub fn run(name: &str, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    run_full(None, name, args, env)?;
    Ok(())
}

pub fn cd_run(cd: &Path, name: &str, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    run_full(Some(cd), name, args, env)?;
    Ok(())
}

pub fn run_full(cd: Option<&Path>, name: &str, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let cmdstr = make_cmdstr(name, args);
    let mut cmd = Command::new(name);

    cmd.args(args);
    for &(k, v) in env {
        cmd.env(k, v);
    }

    if let Some(cd) = cd {
        cmd.current_dir(cd);
    }

    info!("running `{}`", cmdstr);
    let out = log_command(cmd)?;

    if out.status.success() {
        Ok(())
    } else {
        Err(format!("command `{}` failed", cmdstr).into())
    }
}

pub fn run_capture(cd: Option<&Path>,
                   name: &str,
                   args: &[&str],
                   env: &[(&str, &str)])
                   -> Result<(Vec<String>, Vec<String>)> {
    let cmdstr = make_cmdstr(name, args);
    let mut cmd = Command::new(name);

    cmd.args(args);
    for &(k, v) in env {
        cmd.env(k, v);
    }

    if let Some(cd) = cd {
        cmd.current_dir(cd);
    }

    info!("running `{}`", cmdstr);
    let out = log_command_capture(cmd)?;

    if out.status.success() {
        Ok((out.stdout, out.stderr))
    } else {
        Err(format!("command `{}` failed", cmdstr).into())
    }
}

fn make_cmdstr(name: &str, args: &[&str]) -> String {
    assert!(!args.is_empty(), "case not handled");
    format!("{} {}", name, args.join(" "))
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

fn log_command(cmd: Command) -> Result<ProcessOutput> {
    log_command_(cmd, false)
}

fn log_command_capture(cmd: Command) -> Result<ProcessOutput> {
    log_command_(cmd, true)
}

const MAX_TIMEOUT_SECS: u64 = 60 * 10 * 2;
const HEARTBEAT_TIMEOUT_SECS: u64 = 60 * 2;

fn log_command_(mut cmd: Command, capture: bool) -> Result<ProcessOutput> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    let stdout = child.stdout.take().expect("");
    let stderr = child.stderr.take().expect("");

    // Needed for killing after timeout
    let child_id = child.id();

    // Child's stdio needs to produce output to avoid being killed
    let (heartbeat_tx, heartbeat_rx) = mpsc::channel();

    let rx_out = sink(Box::new(stdout),
                      log_child_stdout,
                      capture,
                      heartbeat_tx.clone());
    let rx_err = sink(Box::new(stderr), log_child_stderr, capture, heartbeat_tx);

    #[cfg(unix)]
    fn kill_process(id: u32) {
        use libc::{SIGKILL, kill, pid_t};
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
            _ => panic!(),
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
                _ => panic!(),
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
        info!("process killed after not generating output for {} s",
              HEARTBEAT_TIMEOUT_SECS);
        bail!(ErrorKind::Timeout);
    } else if timed_out {
        info!("process killed after max time of {} s", MAX_TIMEOUT_SECS);
        bail!(ErrorKind::Timeout);
    }

    Ok(ProcessOutput {
           status: status,
           stdout: stdout,
           stderr: stderr,
       })
}

fn log_child_stdout(logger: &Logger, line: &str) {
    slog_info!(logger, "blam! {}", line);
}

fn log_child_stderr(logger: &Logger, line: &str) {
    slog_info!(logger, "kablam! {}", line);
}

fn sink(reader: Box<Read + Send>,
        log: fn(&Logger, &str),
        capture: bool,
        heartbeat_tx: Sender<()>)
        -> Receiver<Vec<String>> {
    let (tx, rx) = channel();
    let logger = slog_scope::logger();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let reader = BufReader::new(reader);
        for line_bytes in reader.split(b'\n') {
            if let Ok(line_bytes) = line_bytes {
                let line = String::from_utf8_lossy(&line_bytes);
                log(&logger, line.deref());
                heartbeat_tx.send(());
                if capture {
                    buf.push(line.to_string());
                }
            } else {
                log(&logger, "READING FROM CHILD PROCESS FAILED!");
            }
        }

        tx.send(buf).expect("");
    });

    rx
}
