use dirs::{CARGO_HOME, RUSTUP_HOME};
use docker::{ContainerBuilder, MountPerms, IMAGE_NAME};
use errors::*;
use futures::{future, Future, Stream};
use futures_cpupool::CpuPool;
use native;
use slog_scope;
use std::convert::AsRef;
use std::env::consts::EXE_SUFFIX;
use std::ffi::{OsStr, OsString};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;
use tokio_core::reactor::Core;
use tokio_io::io::lines;
use tokio_process::CommandExt;
use tokio_timer;
use utils::size::Size;

pub(crate) enum Binary {
    Global(PathBuf),
    InstalledByCrater(PathBuf),
}

pub(crate) trait Runnable {
    fn binary(&self) -> Binary;

    fn prepare_command(&self, cmd: RunCommand) -> RunCommand {
        cmd
    }
}

impl<'a> Runnable for &'a str {
    fn binary(&self) -> Binary {
        Binary::Global(self.into())
    }
}

impl Runnable for String {
    fn binary(&self) -> Binary {
        Binary::Global(self.into())
    }
}

impl<'a, R: Runnable> Runnable for &'a R {
    fn binary(&self) -> Binary {
        Runnable::binary(*self)
    }

    fn prepare_command(&self, cmd: RunCommand) -> RunCommand {
        Runnable::prepare_command(*self, cmd)
    }
}

pub(crate) struct RunCommand {
    binary: Binary,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    cd: Option<PathBuf>,
    quiet: bool,
    enable_timeout: bool,
    local_rustup: bool,
}

impl RunCommand {
    pub(crate) fn new<R: Runnable>(runnable: R) -> Self {
        runnable.prepare_command(RunCommand {
            binary: runnable.binary(),
            args: Vec::new(),
            env: Vec::new(),
            cd: None,
            quiet: false,
            enable_timeout: true,
            local_rustup: false,
        })
    }

    pub(crate) fn args<S: AsRef<OsStr>>(mut self, args: &[S]) -> Self {
        for arg in args {
            self.args.push(arg.as_ref().to_os_string());
        }

        self
    }

    pub(crate) fn env<S1: AsRef<OsStr>, S2: AsRef<OsStr>>(mut self, key: S1, value: S2) -> Self {
        self.env
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    pub(crate) fn cd<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.cd = Some(path.as_ref().to_path_buf());
        self
    }

    pub(crate) fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    pub(crate) fn enable_timeout(mut self, enable_timeout: bool) -> Self {
        self.enable_timeout = enable_timeout;
        self
    }

    pub(crate) fn local_rustup(mut self, local_rustup: bool) -> Self {
        self.local_rustup = local_rustup;
        self
    }

    pub(crate) fn sandboxed(self) -> SandboxedCommand {
        SandboxedCommand::new(self)
    }

    pub(crate) fn run(self) -> Result<()> {
        self.run_inner(false)?;
        Ok(())
    }

    pub(crate) fn run_capture(self) -> Result<(Vec<String>, Vec<String>)> {
        let out = self.run_inner(true)?;
        Ok((out.stdout, out.stderr))
    }

    fn run_inner(self, capture: bool) -> Result<ProcessOutput> {
        let name = match self.binary {
            Binary::Global(path) => path,
            Binary::InstalledByCrater(path) => ::utils::fs::try_canonicalize(format!(
                "{}/bin/{}{}",
                *CARGO_HOME,
                path.to_string_lossy(),
                EXE_SUFFIX
            )),
        };

        let mut cmd = Command::new(&name);

        cmd.args(&self.args);

        if self.local_rustup {
            cmd.env("CARGO_HOME", ::utils::fs::try_canonicalize(&*CARGO_HOME));
            cmd.env("RUSTUP_HOME", ::utils::fs::try_canonicalize(&*RUSTUP_HOME));
        }
        for &(ref k, ref v) in &self.env {
            cmd.env(k, v);
        }

        let cmdstr = format!("{:?}", cmd);

        if let Some(ref cd) = self.cd {
            cmd.current_dir(cd);
        }

        info!("running `{}`", cmdstr);
        let out = log_command(cmd, capture, self.quiet, self.enable_timeout).map_err(|e| {
            info!("error running command: {}", e);
            e
        })?;

        if out.status.success() {
            Ok(out)
        } else {
            Err(format!("command `{}` failed", cmdstr).into())
        }
    }
}

pub(crate) struct SandboxedCommand {
    command: RunCommand,
    container: ContainerBuilder,
}

impl SandboxedCommand {
    fn new(command: RunCommand) -> Self {
        let container = ContainerBuilder::new(IMAGE_NAME)
            .env("USER_ID", native::current_user().to_string())
            .enable_networking(false);

        SandboxedCommand { command, container }
    }

    pub(crate) fn memory_limit(mut self, limit: Option<Size>) -> Self {
        self.container = self.container.memory_limit(limit);
        self
    }

    pub(crate) fn mount<P1: Into<PathBuf>, P2: Into<PathBuf>>(
        mut self,
        host_path: P1,
        container_path: P2,
        perm: MountPerms,
    ) -> Self {
        self.container = self.container.mount(host_path, container_path, perm);
        self
    }

    pub(crate) fn run(mut self) -> Result<()> {
        // Build the full CLI
        let mut cmd = match self.command.binary {
            Binary::Global(path) => path,
            Binary::InstalledByCrater(path) => path,
        }.to_string_lossy()
        .as_ref()
        .to_string();
        for arg in self.command.args {
            cmd.push(' ');
            cmd.push_str(arg.to_string_lossy().as_ref());
        }

        let source_dir = match self.command.cd {
            Some(path) => path,
            None => PathBuf::from("."),
        };

        self.container = self
            .container
            .mount(source_dir, "/source", MountPerms::ReadOnly)
            .env("SOURCE_DIR", "/source")
            .env("USER_ID", native::current_user().to_string())
            .env("CMD", cmd);

        for (key, value) in self.command.env {
            self.container = self.container.env(
                key.to_string_lossy().as_ref(),
                value.to_string_lossy().as_ref(),
            );
        }

        if self.command.local_rustup {
            self.container = self
                .container
                .mount(&*CARGO_HOME, "/cargo-home", MountPerms::ReadOnly)
                .mount(&*RUSTUP_HOME, "/rustup-home", MountPerms::ReadOnly)
                .env("CARGO_HOME", "/cargo-home")
                .env("RUSTUP_HOME", "/rustup-home");
        }

        self.container.run(self.command.quiet)
    }
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

const MAX_TIMEOUT_SECS: u64 = 60 * 15;
const HEARTBEAT_TIMEOUT_SECS: u64 = 60 * 5;

fn log_command(
    mut cmd: Command,
    capture: bool,
    quiet: bool,
    enable_timeout: bool,
) -> Result<ProcessOutput> {
    let (max_timeout, heartbeat_timeout) = if enable_timeout {
        let max_timeout = Duration::from_secs(MAX_TIMEOUT_SECS);
        let heartbeat_timeout = if quiet {
            // If the command is known to be slow, the heartbeat timeout is set to the same value as
            // the max timeout, so it can't be triggered.
            max_timeout
        } else {
            Duration::from_secs(HEARTBEAT_TIMEOUT_SECS)
        };

        (max_timeout, heartbeat_timeout)
    } else {
        // If timeouts are disabled just use a *really* long timeout
        let max = Duration::from_secs(7 * 24 * 60 * 60);
        (max, max)
    };

    let mut core = Core::new().unwrap();
    let timer = tokio_timer::wheel().max_timeout(max_timeout * 2).build();
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_async(&core.handle())?;

    let stdout = child.stdout().take().expect("");
    let stderr = child.stderr().take().expect("");

    // Needed for killing after timeout
    let child_id = child.id();

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
                match native::kill_process(child_id) {
                    Err(err) => err,
                    Ok(()) => Error::from(ErrorKind::Timeout(
                        "not generating output for ",
                        heartbeat_timeout.as_secs(),
                    )),
                }
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

    let child = timer.timeout(child, max_timeout).map_err(move |e| {
        if e.kind() == io::ErrorKind::TimedOut {
            match native::kill_process(child_id) {
                Err(err) => err,
                Ok(()) => ErrorKind::Timeout("max time of", MAX_TIMEOUT_SECS).into(),
            }
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
        status,
        stdout,
        stderr,
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
            }).fold((Vec::new(), Vec::new()), |mut v, i| {
                if let Some(i) = i.0 {
                    v.0.push(i);
                }
                if let Some(i) = i.1 {
                    v.1.push(i);
                }
                Ok(v)
            }),
    )
}
