use dirs::{CARGO_HOME, RUSTUP_HOME};
use docker::{ContainerBuilder, DockerEnv, MountPerms};
use failure::Error;
use futures::{future, Future, Stream};
use native;
use prelude::*;
use slog_scope;
use std::convert::AsRef;
use std::env::consts::EXE_SUFFIX;
use std::ffi::{OsStr, OsString};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use tokio::{io::lines, runtime::current_thread::block_on_all, util::*};
use tokio_process::CommandExt;
use utils::size::Size;

#[derive(Debug, Fail)]
pub enum RunCommandError {
    #[fail(display = "no output for {} seconds", _0)]
    NoOutputFor(u64),
    #[fail(display = "command timed out after {} seconds", _0)]
    Timeout(u64),
}

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
    hide_output: bool,
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
            hide_output: false,
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

    pub(crate) fn hide_output(mut self, hide_output: bool) -> Self {
        self.hide_output = hide_output;
        self
    }

    pub(crate) fn sandboxed(self, docker_env: &DockerEnv) -> SandboxedCommand {
        SandboxedCommand::new(self, docker_env)
    }

    pub(crate) fn run(self) -> Fallible<()> {
        self.run_inner(false)?;
        Ok(())
    }

    pub(crate) fn run_capture(self) -> Fallible<(Vec<String>, Vec<String>)> {
        let out = self.run_inner(true)?;
        Ok((out.stdout, out.stderr))
    }

    fn run_inner(self, capture: bool) -> Fallible<ProcessOutput> {
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
        let out = log_command(
            cmd,
            capture,
            self.quiet,
            self.enable_timeout,
            self.hide_output,
        ).map_err(|e| {
            error!("error running command: {}", e);
            e
        })?;

        if out.status.success() {
            Ok(out)
        } else {
            bail!("command `{}` failed", cmdstr);
        }
    }
}

pub(crate) struct SandboxedCommand {
    command: RunCommand,
    container: ContainerBuilder,
}

impl SandboxedCommand {
    fn new(command: RunCommand, docker_env: &DockerEnv) -> Self {
        let container = ContainerBuilder::new(docker_env)
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

    pub(crate) fn run(mut self) -> Fallible<()> {
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

enum OutputKind {
    Stdout,
    Stderr,
}

impl OutputKind {
    fn prefix(&self) -> &'static str {
        match *self {
            OutputKind::Stdout => "stdout",
            OutputKind::Stderr => "stderr",
        }
    }
}

const MAX_TIMEOUT_SECS: u64 = 60 * 15;
const HEARTBEAT_TIMEOUT_SECS: u64 = 60 * 5;

fn log_command(
    mut cmd: Command,
    capture: bool,
    quiet: bool,
    enable_timeout: bool,
    hide_output: bool,
) -> Fallible<ProcessOutput> {
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

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_async()?;
    let child_id = child.id();

    let stdout = lines(BufReader::new(child.stdout().take().unwrap()))
        .map(|line| (OutputKind::Stdout, line));
    let stderr = lines(BufReader::new(child.stderr().take().unwrap()))
        .map(|line| (OutputKind::Stderr, line));

    let start = Instant::now();

    let logger = slog_scope::logger();
    let output = stdout
        .select(stderr)
        .timeout(heartbeat_timeout)
        .map_err(move |err| {
            if err.is_elapsed() {
                match native::kill_process(child_id) {
                    Ok(()) => {
                        Error::from(RunCommandError::NoOutputFor(heartbeat_timeout.as_secs()))
                    }
                    Err(err) => err,
                }
            } else {
                Error::from(err)
            }
        }).and_then(move |(kind, line)| {
            // If the process is in a tight output loop the timeout on the process might fail to
            // be executed, so this extra check prevents the process to run without limits.
            if start.elapsed() > max_timeout {
                return future::err(Error::from(RunCommandError::Timeout(max_timeout.as_secs())));
            }

            if !hide_output {
                slog_info!(logger, "[{}] {}", kind.prefix(), line);
            }
            future::ok((kind, line))
        }).fold(
            (Vec::new(), Vec::new()),
            move |mut res, (kind, line)| -> Fallible<_> {
                if capture {
                    match kind {
                        OutputKind::Stdout => res.0.push(line),
                        OutputKind::Stderr => res.1.push(line),
                    }
                }
                Ok(res)
            },
        );

    let child = child.timeout(max_timeout).map_err(move |err| {
        if err.is_elapsed() {
            match native::kill_process(child_id) {
                Ok(()) => Error::from(RunCommandError::Timeout(max_timeout.as_secs())),
                Err(err) => err,
            }
        } else {
            Error::from(err)
        }
    });

    let ((stdout, stderr), status) = block_on_all(output.join(child))?;

    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
    })
}
