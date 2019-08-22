//! Command execution and sandboxing.

mod sandbox;

pub use sandbox::*;

use crate::native;
use crate::workspace::Workspace;
use failure::{Error, Fail};
use futures::{future, Future, Stream};
use log::{error, info};
use std::convert::AsRef;
use std::env::consts::EXE_SUFFIX;
use std::ffi::{OsStr, OsString};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use tokio::{io::lines, runtime::current_thread::block_on_all, util::*};
use tokio_process::CommandExt;

pub(crate) mod container_dirs {
    use std::path::{Path, PathBuf};

    use lazy_static::lazy_static;

    #[cfg(windows)]
    lazy_static! {
        pub(super) static ref ROOT_DIR: PathBuf = Path::new(r"C:\crater").into();
    }

    #[cfg(not(windows))]
    lazy_static! {
        pub(super) static ref ROOT_DIR: PathBuf = Path::new("/opt/crater").into();
    }

    lazy_static! {
        pub(crate) static ref WORK_DIR: PathBuf = ROOT_DIR.join("workdir");
        pub(crate) static ref TARGET_DIR: PathBuf = ROOT_DIR.join("target");
        pub(super) static ref CARGO_HOME: PathBuf = ROOT_DIR.join("cargo-home");
        pub(super) static ref RUSTUP_HOME: PathBuf = ROOT_DIR.join("rustup-home");
        pub(super) static ref CARGO_BIN_DIR: PathBuf = CARGO_HOME.join("bin");
    }
}

/// Error happened while executing a command.
#[derive(Debug, Fail)]
pub enum CommandError {
    /// The command didn't output anything to stdout or stderr for more than the timeout, and it
    /// was killed. The timeout's value (in seconds) is the first value.
    #[fail(display = "no output for {} seconds", _0)]
    NoOutputFor(u64),
    /// The command took more time than the timeout to end, and it was killed. The timeout's value
    /// (in seconds) is the first value.
    #[fail(display = "command timed out after {} seconds", _0)]
    Timeout(u64),
    /// The sandbox ran out of memory and was killed.
    #[fail(display = "container ran out of memory")]
    SandboxOOM,
    #[doc(hidden)]
    #[fail(display = "this error shouldn't have happened")]
    __NonExaustive,
}

/// Name and kind of a binary executed by [`Command`](struct.Command.html).
pub enum Binary {
    /// Global binary, available in `$PATH`. Rustwide doesn't apply any tweaks to its execution
    /// environment.
    Global(PathBuf),
    /// Binary installed and managed by Rustwide in its local rustup installation. Rustwide will
    /// tweak the environment to use the local rustup instead of the host system one, and will
    /// search the binary in the cargo home.
    ManagedByRustwide(PathBuf),
}

/// Trait representing a command that can be run by [`Command`](struct.Command.html).
pub trait Runnable {
    /// The name of the binary to execute.
    fn name(&self) -> Binary;

    /// Prepare the command for execution. This method is called as soon as a
    /// [`Command`](struct.Command.html) instance is created, and allows tweaking the command to
    /// better suit your binary, for example by adding default arguments or environment variables.
    ///
    /// The default implementation simply returns the provided command without changing anything in
    /// it.
    fn prepare_command<'w, 'pl>(&self, cmd: Command<'w, 'pl>) -> Command<'w, 'pl> {
        cmd
    }
}

impl<'a> Runnable for &'a str {
    fn name(&self) -> Binary {
        Binary::Global(self.into())
    }
}

impl Runnable for String {
    fn name(&self) -> Binary {
        Binary::Global(self.into())
    }
}

impl<'a, B: Runnable> Runnable for &'a B {
    fn name(&self) -> Binary {
        Runnable::name(*self)
    }

    fn prepare_command<'w, 'pl>(&self, cmd: Command<'w, 'pl>) -> Command<'w, 'pl> {
        Runnable::prepare_command(*self, cmd)
    }
}

/// The `Command` is a builder to execute system commands and interact with them.
///
/// It's a more advanced version of [`std::process::Command`][std], featuring timeouts, realtime
/// output processing, output logging and sandboxing.
///
/// [std]: https://doc.rust-lang.org/std/process/struct.Command.html
pub struct Command<'w, 'pl> {
    workspace: &'w Workspace,
    sandbox: Option<SandboxBuilder>,
    binary: Binary,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    process_lines: Option<&'pl mut dyn FnMut(&str)>,
    cd: Option<PathBuf>,
    timeout: Option<Duration>,
    no_output_timeout: Option<Duration>,
    log_output: bool,
}

impl<'w, 'pl> Command<'w, 'pl> {
    /// Create a new, unsandboxed command.
    pub fn new<R: Runnable>(workspace: &'w Workspace, binary: R) -> Self {
        binary.prepare_command(Self::new_inner(binary.name(), workspace, None))
    }

    /// Create a new, sandboxed command.
    pub fn new_sandboxed<R: Runnable>(
        workspace: &'w Workspace,
        sandbox: SandboxBuilder,
        binary: R,
    ) -> Self {
        binary.prepare_command(Self::new_inner(binary.name(), workspace, Some(sandbox)))
    }

    fn new_inner(
        binary: Binary,
        workspace: &'w Workspace,
        sandbox: Option<SandboxBuilder>,
    ) -> Self {
        Command {
            workspace,
            sandbox,
            binary,
            args: Vec::new(),
            env: Vec::new(),
            process_lines: None,
            cd: None,
            timeout: workspace.default_command_timeout(),
            no_output_timeout: workspace.default_command_no_output_timeout(),
            log_output: true,
        }
    }

    /// Add command-line arguments to the command. This method can be called multiple times to add
    /// additional args.
    pub fn args<S: AsRef<OsStr>>(mut self, args: &[S]) -> Self {
        for arg in args {
            self.args.push(arg.as_ref().to_os_string());
        }

        self
    }

    /// Add an environment variable to the command.
    pub fn env<S1: AsRef<OsStr>, S2: AsRef<OsStr>>(mut self, key: S1, value: S2) -> Self {
        self.env
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    /// Change the directory where the command will be executed in.
    pub fn cd<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.cd = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set the timeout of this command. If it runs for more time the process will be killed.
    ///
    /// Its default value is configured through
    /// [`WorkspaceBuilder::command_timeout`](../struct.WorkspaceBuilder.html#method.command_timeout).
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the no output timeout of this command. If it doesn't output anything for more time the
    /// process will be killed.
    ///
    /// Its default value is configured through
    /// [`WorkspaceBuilder::command_no_output_timeout`](../struct.WorkspaceBuilder.html#method.command_no_output_timeout).
    pub fn no_output_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.no_output_timeout = timeout;
        self
    }

    /// Set the function that will be called each time a line is outputted to either the standard
    /// output or the standard error. Only one function can be set at any time for a command.
    ///
    /// The method is useful to analyze the command's output without storing all of it in memory.
    /// This example builds a crate and detects compiler errors (ICEs):
    ///
    /// ```no_run
    /// let mut ice = false;
    /// Command::new(workspace, "cargo")
    ///     .args(&["build", "--all"])
    ///     .process_lines(|line| {
    ///         if line.contains("internal compiler error") {
    ///             ice = true;
    ///         }
    ///     })
    ///     .run()?;
    /// ```
    pub fn process_lines(mut self, f: &'pl mut dyn FnMut(&str)) -> Self {
        self.process_lines = Some(f);
        self
    }

    /// Enable or disable logging all the output lines to the [`log` crate][log]. By default
    /// logging is enabled.
    ///
    /// [log]: (https://crates.io/crates/log)
    pub fn log_output(mut self, log_output: bool) -> Self {
        self.log_output = log_output;
        self
    }

    /// Run the prepared command and return an error if it fails (for example with a non-zero exit
    /// code or a timeout).
    pub fn run(self) -> Result<(), Error> {
        self.run_inner(false)?;
        Ok(())
    }

    /// Run the prepared command and return its output if it succeedes. If it fails (for example
    /// with a non-zero exit code or a timeout) an error will be returned instead.
    ///
    /// Even though the output will be captured and returned, if output logging is enabled (as it
    /// is by default) the output will be also logged. You can disable this behavior by calling the
    /// [`log_output`](struct.Command.html#method.log_output) method.
    pub fn run_capture(self) -> Result<ProcessOutput, Error> {
        Ok(self.run_inner(true)?)
    }

    fn run_inner(self, capture: bool) -> Result<ProcessOutput, Error> {
        if let Some(mut builder) = self.sandbox {
            let binary = match self.binary {
                Binary::Global(path) => path,
                Binary::ManagedByRustwide(path) => {
                    container_dirs::CARGO_BIN_DIR.join(exe_suffix(path.as_os_str()))
                }
            };

            let mut cmd = Vec::new();
            cmd.push(binary.to_string_lossy().as_ref().to_string());

            for arg in self.args {
                cmd.push(arg.to_string_lossy().to_string());
            }

            let source_dir = match self.cd {
                Some(path) => path,
                None => PathBuf::from("."),
            };

            builder = builder
                .mount(&source_dir, &*container_dirs::WORK_DIR, MountKind::ReadOnly)
                .env("SOURCE_DIR", container_dirs::WORK_DIR.to_str().unwrap())
                .workdir(container_dirs::WORK_DIR.to_str().unwrap())
                .cmd(cmd);

            if let Some(user_id) = native::current_user() {
                builder = builder.env("MAP_USER_ID", user_id.to_string());
            }

            for (key, value) in self.env {
                builder = builder.env(
                    key.to_string_lossy().as_ref(),
                    value.to_string_lossy().as_ref(),
                );
            }

            builder = builder
                .mount(
                    &self.workspace.cargo_home(),
                    &*container_dirs::CARGO_HOME,
                    MountKind::ReadOnly,
                )
                .mount(
                    &self.workspace.rustup_home(),
                    &*container_dirs::RUSTUP_HOME,
                    MountKind::ReadOnly,
                )
                .env("CARGO_HOME", container_dirs::CARGO_HOME.to_str().unwrap())
                .env("RUSTUP_HOME", container_dirs::RUSTUP_HOME.to_str().unwrap());

            builder.run(self.workspace, self.timeout, self.no_output_timeout)?;
            Ok(ProcessOutput {
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        } else {
            let (binary, managed_by_rustwide) = match self.binary {
                Binary::Global(path) => (path, false),
                Binary::ManagedByRustwide(path) => (
                    self.workspace
                        .cargo_home()
                        .join("bin")
                        .join(exe_suffix(path.as_os_str())),
                    true,
                ),
            };
            let mut cmd = StdCommand::new(crate::utils::normalize_path(&binary));

            cmd.args(&self.args);

            if managed_by_rustwide {
                let cargo_home = self
                    .workspace
                    .cargo_home()
                    .to_str()
                    .expect("bad cargo home")
                    .to_string();
                let rustup_home = self
                    .workspace
                    .rustup_home()
                    .to_str()
                    .expect("bad rustup home")
                    .to_string();
                cmd.env(
                    "CARGO_HOME",
                    crate::utils::normalize_path(cargo_home.as_ref()),
                );
                cmd.env(
                    "RUSTUP_HOME",
                    crate::utils::normalize_path(rustup_home.as_ref()),
                );
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
                self.process_lines,
                capture,
                self.timeout,
                self.no_output_timeout,
                self.log_output,
            )
            .map_err(|e| {
                error!("error running command: {}", e);
                e
            })?;

            if out.status.success() {
                Ok(out.into())
            } else {
                failure::bail!("command `{}` failed", cmdstr);
            }
        }
    }
}

struct InnerProcessOutput {
    status: ExitStatus,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

impl From<InnerProcessOutput> for ProcessOutput {
    fn from(orig: InnerProcessOutput) -> ProcessOutput {
        ProcessOutput {
            stdout: orig.stdout,
            stderr: orig.stderr,
        }
    }
}

/// Output of a [`Command`](struct.Command.html) when it was executed with the
/// [`run_capture`](struct.Command.html#method.run_capture) method.
pub struct ProcessOutput {
    stdout: Vec<String>,
    stderr: Vec<String>,
}

impl ProcessOutput {
    /// Return a list of the lines printed by the process on the standard output.
    pub fn stdout_lines(&self) -> &[String] {
        &self.stdout
    }

    /// Return a list of the lines printed by the process on the standard error.
    pub fn stderr_lines(&self) -> &[String] {
        &self.stderr
    }
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

fn log_command(
    mut cmd: StdCommand,
    mut process_lines: Option<&mut dyn FnMut(&str)>,
    capture: bool,
    timeout: Option<Duration>,
    no_output_timeout: Option<Duration>,
    log_output: bool,
) -> Result<InnerProcessOutput, Error> {
    let timeout = if let Some(t) = timeout {
        t
    } else {
        // If timeouts are disabled just use a *really* long timeout
        // FIXME: this hack is horrible
        Duration::from_secs(7 * 24 * 60 * 60)
    };
    let no_output_timeout = if let Some(t) = no_output_timeout {
        t
    } else {
        // If the no output timeout is disabled set it the same as the full timeout.
        timeout
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

    let output = stdout
        .select(stderr)
        .timeout(no_output_timeout)
        .map_err(move |err| {
            if err.is_elapsed() {
                match native::kill_process(child_id) {
                    Ok(()) => Error::from(CommandError::NoOutputFor(no_output_timeout.as_secs())),
                    Err(err) => err,
                }
            } else {
                Error::from(err)
            }
        })
        .and_then(move |(kind, line)| {
            // If the process is in a tight output loop the timeout on the process might fail to
            // be executed, so this extra check prevents the process to run without limits.
            if start.elapsed() > timeout {
                return future::err(Error::from(CommandError::Timeout(timeout.as_secs())));
            }

            if log_output {
                info!("[{}] {}", kind.prefix(), line);
            }
            future::ok((kind, line))
        })
        .fold(
            (Vec::new(), Vec::new()),
            move |mut res, (kind, line)| -> Result<_, Error> {
                if let Some(f) = &mut process_lines {
                    f(&line);
                }
                if capture {
                    match kind {
                        OutputKind::Stdout => res.0.push(line),
                        OutputKind::Stderr => res.1.push(line),
                    }
                }
                Ok(res)
            },
        );

    let child = child.timeout(timeout).map_err(move |err| {
        if err.is_elapsed() {
            match native::kill_process(child_id) {
                Ok(()) => Error::from(CommandError::Timeout(timeout.as_secs())),
                Err(err) => err,
            }
        } else {
            Error::from(err)
        }
    });

    let ((stdout, stderr), status) = block_on_all(output.join(child))?;

    Ok(InnerProcessOutput {
        status,
        stdout,
        stderr,
    })
}

fn exe_suffix(file: &OsStr) -> OsString {
    let mut path = OsString::from(file);
    path.push(EXE_SUFFIX);
    path
}
