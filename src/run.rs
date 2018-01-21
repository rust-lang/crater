#![deny(unused_must_use)]

use errors::*;
use futures::{future, Future, Stream};
use futures_cpupool::CpuPool;
use slog_scope;
use std::convert::AsRef;
use std::ffi::OsStr;
use std::io::{self, BufReader};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;
use tokio_core::reactor::Core;
use tokio_io::io::lines;
use tokio_process::CommandExt;
use tokio_timer;

pub fn run(name: &str, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    run_full(None, name, args, env)?;
    Ok(())
}

pub fn cd_run(cd: &Path, name: &str, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    run_full(Some(cd), name, args, env)?;
    Ok(())
}

pub fn run_full(cd: Option<&Path>, name: &str, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let mut cmd = Command::new(name);

    cmd.args(args);
    for &(k, v) in env {
        cmd.env(k, v);
    }
    let cmdstr = format!{"{:?}", cmd};

    if let Some(cd) = cd {
        cmd.current_dir(cd);
    }

    info!("running `{}`", cmdstr);
    let out = log_command(cmd).map_err(|e| {
        info!("error running command: {}", e);
        e
    })?;

    if out.status.success() {
        Ok(())
    } else {
        Err(format!("command `{}` failed", cmdstr).into())
    }
}

pub fn run_capture<S>(
    cd: Option<&Path>,
    name: &str,
    args: &[S],
    env: &[(&str, &str)],
) -> Result<(Vec<String>, Vec<String>)>
where
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(name);

    cmd.args(args);
    for &(k, v) in env {
        cmd.env(k, v);
    }

    let cmdstr = format!{"{:?}", cmd};

    if let Some(cd) = cd {
        cmd.current_dir(cd);
    }

    info!("running `{}`", cmdstr);
    let out = log_command_capture(cmd).map_err(|e| {
        info!("error running command: {}", e);
        e
    })?;

    if out.status.success() {
        Ok((out.stdout, out.stderr))
    } else {
        Err(format!("command `{}` failed", cmdstr).into())
    }
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

const MAX_TIMEOUT_SECS: u64 = 60 * 15;
const HEARTBEAT_TIMEOUT_SECS: u64 = 60 * 2;

fn log_command_(mut cmd: Command, capture: bool) -> Result<ProcessOutput> {
    let mut core = Core::new().unwrap();
    let timer = tokio_timer::wheel()
        .max_timeout(Duration::from_secs(MAX_TIMEOUT_SECS * 2))
        .build();
    let mut child = cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_async(&core.handle())?;

    let stdout = child.stdout().take().expect("");
    let stderr = child.stderr().take().expect("");

    // Needed for killing after timeout
    let child_id = child.id();

    let heartbeat_timeout = Duration::from_secs(HEARTBEAT_TIMEOUT_SECS);
    let max_timeout = Duration::from_secs(MAX_TIMEOUT_SECS);

    let logger = slog_scope::logger();
    let stdout = lines(BufReader::new(stdout)).map({
        let logger = logger.clone();
        move |line| {
            slog_info!(logger, "blam! {}", line);
            line
        }
    });
    let stderr = lines(BufReader::new(stderr)).map({
        let logger = logger.clone();
        move |line| {
            slog_info!(logger, "kablam! {}", line);
            line
        }
    });

    let output = Stream::select(stdout.map(future::Either::A), stderr.map(future::Either::B));
    let output = timer
        .timeout_stream(output, heartbeat_timeout)
        .map_err(move |e| {
            if e.kind() == io::ErrorKind::TimedOut {
                kill_process(child_id);
                Error::from(ErrorKind::Timeout(
                    "not generating output for ",
                    HEARTBEAT_TIMEOUT_SECS,
                ))
            } else {
                e.into()
            }
        });

    let output = if capture {
        unmerge(output)
    } else {
        Box::new(
            output
                .for_each(|_| Ok(()))
                .and_then(|_| Ok((Vec::new(), Vec::new()))),
        )
    };
    let pool = CpuPool::new(1);
    let output = pool.spawn(output);

    #[cfg(unix)]
    fn kill_process(id: u32) {
        use libc::{kill, pid_t, SIGKILL};
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

    let child = timer.timeout(child, max_timeout).map_err(move |e| {
        if e.kind() == io::ErrorKind::TimedOut {
            kill_process(child_id);
            ErrorKind::Timeout("max time of", MAX_TIMEOUT_SECS).into()
        } else {
            e.into()
        }
    });

    // TODO: Handle errors from tokio_timer better, in particular TimerError::TooLong
    let (status, (stdout, stderr)) = core.run(child.select2(output).then(|res| {
        let future: Box<Future<Item = _, Error = _>> = match res {
            // child exited, finish collecting output
            Ok(future::Either::A((status, output))) => {
                Box::new(output.map(move |sose| (status, sose)))
            }
            // output finished, wait for process to exit (possibly being killed by timeout)
            Ok(future::Either::B((sose, child))) => {
                Box::new(child.map(move |status| (status, sose)))
            }
            // child lived too long and was killed, finish collecting output so it goes to logs then
            // return timeout error (not interested in errors with output at this point, so ignore)
            Err(future::Either::A((e, output))) => Box::new(output.then(|_| future::err(e))),
            // output collection failed (timeout, misc io error) and child was killed, drop timeout
            Err(future::Either::B((e, _child))) => Box::new(future::err(e)),
        };
        future
    }))?;

    Ok(ProcessOutput {
        status: status,
        stdout: stdout,
        stderr: stderr,
    })
}

#[cfg_attr(feature = "cargo-clippy", allow(type_complexity))]
fn unmerge<T1, T2, S>(reader: S) -> Box<Future<Item = (Vec<T1>, Vec<T2>), Error = S::Error> + Send>
where
    S: Stream<Item = future::Either<T1, T2>> + Send + 'static,
    S::Error: Send,
    T1: Send + 'static,
    T2: Send + 'static,
{
    Box::new(
        reader
            .map(|i| match i {
                future::Either::A(l) => (Some(l), None),
                future::Either::B(r) => (None, Some(r)),
            })
            .fold((Vec::new(), Vec::new()), |mut v, i| {
                i.0.map(|i| v.0.push(i));
                i.1.map(|i| v.1.push(i));
                Ok(v)
            }),
    )
}
