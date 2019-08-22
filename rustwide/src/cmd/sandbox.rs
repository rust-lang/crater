use crate::cmd::{Command, CommandError};
use crate::Workspace;
use failure::Error;
use log::{error, info};
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

/// The Docker image used for sandboxing.
pub struct SandboxImage {
    name: String,
}

impl SandboxImage {
    /// Load a local image present in the host machine.
    ///
    /// If the image is not available locally an error will be returned instead.
    pub fn local(name: &str) -> Result<Self, Error> {
        let image = SandboxImage { name: name.into() };
        info!("sandbox image is local, skipping pull");
        image.ensure_exists_locally()?;
        Ok(image)
    }

    /// Pull an image from its Docker registry.
    ///
    /// This will access the network to download the image from the registry. If pulling fails an
    /// error will be returned instead.
    pub fn remote(name: &str) -> Result<Self, Error> {
        let image = SandboxImage { name: name.into() };
        info!("pulling image {} from Docker Hub", name);
        let status = StdCommand::new("docker").args(&["pull", &name]).status()?;
        if !status.success() {
            failure::bail!("failed to pull image {} from Docker Hub", name);
        }
        image.ensure_exists_locally()?;
        Ok(image)
    }

    fn ensure_exists_locally(&self) -> Result<(), Error> {
        info!("checking the image {} is available locally", self.name);
        let out = StdCommand::new("docker")
            .args(&["image", "inspect", &self.name])
            .output()?;
        if !out.status.success() {
            failure::bail!("the docker image {} is not available locally", self.name);
        }
        Ok(())
    }
}

/// Whether to mount a path in the sandbox with write permissions or not.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MountKind {
    /// Allow the sandboxed code to change the mounted data.
    ReadWrite,
    /// Prevent the sandboxed code from changing the mounted data.
    ReadOnly,
}

#[derive(Clone)]
struct MountConfig {
    host_path: PathBuf,
    sandbox_path: PathBuf,
    perm: MountKind,
}

impl MountConfig {
    fn to_volume_arg(&self) -> String {
        let perm = match self.perm {
            MountKind::ReadWrite => "rw",
            MountKind::ReadOnly => "ro",
        };
        format!(
            "{}:{}:{},Z",
            absolute(&self.host_path).to_string_lossy(),
            self.sandbox_path.to_string_lossy(),
            perm
        )
    }

    fn to_mount_arg(&self) -> String {
        let mut opts_with_leading_comma = vec![];

        if self.perm == MountKind::ReadOnly {
            opts_with_leading_comma.push(",readonly");
        }

        format!(
            "type=bind,src={},dst={}{}",
            absolute(&self.host_path).to_string_lossy(),
            self.sandbox_path.to_string_lossy(),
            opts_with_leading_comma.join(""),
        )
    }
}

/// The sandbox builder allows to configure a sandbox, used later in a
/// [`Command`](struct.Command.html).
#[derive(Clone)]
pub struct SandboxBuilder {
    mounts: Vec<MountConfig>,
    env: Vec<(String, String)>,
    memory_limit: Option<usize>,
    workdir: Option<String>,
    cmd: Vec<String>,
    enable_networking: bool,
}

impl SandboxBuilder {
    /// Create a new sandbox builder.
    pub fn new() -> Self {
        Self {
            mounts: Vec::new(),
            env: Vec::new(),
            workdir: None,
            memory_limit: None,
            cmd: Vec::new(),
            enable_networking: true,
        }
    }

    /// Mount a path inside the sandbox. It's possible to choose whether to mount the path
    /// read-only or writeable through the [`MountKind`](enum.MountKind.html) enum.
    pub fn mount(mut self, host_path: &Path, sandbox_path: &Path, kind: MountKind) -> Self {
        self.mounts.push(MountConfig {
            host_path: host_path.into(),
            sandbox_path: sandbox_path.into(),
            perm: kind,
        });
        self
    }

    /// Enable or disable the sandbox's memory limit. When the processes inside the sandbox use
    /// more memory than the limit the sandbox will be killed.
    ///
    /// By default no memory limit is present, and its size is provided in bytes.
    pub fn memory_limit(mut self, limit: Option<usize>) -> Self {
        self.memory_limit = limit;
        self
    }

    /// Enable or disable the sandbox's networking. When it's disabled processes inside the sandbox
    /// won't be able to reach network service on the Internet or the host machine.
    ///
    /// By default networking is enabled.
    pub fn enable_networking(mut self, enable: bool) -> Self {
        self.enable_networking = enable;
        self
    }

    pub(super) fn env<S1: Into<String>, S2: Into<String>>(mut self, key: S1, value: S2) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub(super) fn cmd(mut self, cmd: Vec<String>) -> Self {
        self.cmd = cmd;
        self
    }

    pub(super) fn workdir<S: Into<String>>(mut self, workdir: S) -> Self {
        self.workdir = Some(workdir.into());
        self
    }

    fn create(self, workspace: &Workspace) -> Result<Container<'_>, Error> {
        let mut args: Vec<String> = vec!["create".into()];

        for mount in &self.mounts {
            std::fs::create_dir_all(&mount.host_path)?;

            // On Windows, we mount paths containing a colon which don't work with `-v`, but on
            // Linux we need the Z flag, which doesn't work with `--mount`, for SELinux relabeling.
            if cfg!(windows) {
                args.push("--mount".into());
                args.push(mount.to_mount_arg())
            } else {
                args.push("-v".into());
                args.push(mount.to_volume_arg())
            }
        }

        for &(ref var, ref value) in &self.env {
            args.push("-e".into());
            args.push(format! {"{}={}", var, value})
        }

        if let Some(workdir) = self.workdir {
            args.push("-w".into());
            args.push(workdir);
        }

        if let Some(limit) = self.memory_limit {
            args.push("-m".into());
            args.push(limit.to_string());
        }

        if !self.enable_networking {
            args.push("--network".into());
            args.push("none".into());
        }

        if cfg!(windows) {
            args.push("--isolation=process".into());
        }

        args.push(workspace.sandbox_image().name.clone());

        for arg in self.cmd {
            args.push(arg);
        }

        let out = Command::new(workspace, "docker")
            .args(&*args)
            .run_capture()?;
        Ok(Container {
            id: out.stdout_lines()[0].clone(),
            workspace,
        })
    }

    pub(super) fn run(
        self,
        workspace: &Workspace,
        timeout: Option<Duration>,
        no_output_timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let container = self.create(workspace)?;

        // Ensure the container is properly deleted even if something panics
        scopeguard::defer! {{
            if let Err(err) = container.delete() {
                error!("failed to delete container {}", container.id);
                error!("caused by: {}", err);
                for cause in err.iter_causes() {
                    error!("caused by: {}", cause);
                }
            }
        }}

        container.run(timeout, no_output_timeout)?;
        Ok(())
    }
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        let cd = std::env::current_dir().expect("unable to get current dir");
        cd.join(path)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectContainer {
    state: InspectState,
}

#[derive(Deserialize)]
struct InspectState {
    #[serde(rename = "OOMKilled")]
    oom_killed: bool,
}

#[derive(Clone)]
struct Container<'w> {
    // Docker container ID
    id: String,
    workspace: &'w Workspace,
}

impl fmt::Display for Container<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

impl Container<'_> {
    fn inspect(&self) -> Result<InspectContainer, Error> {
        let output = Command::new(self.workspace, "docker")
            .args(&["inspect", &self.id])
            .log_output(false)
            .run_capture()?;

        let mut data: Vec<InspectContainer> =
            ::serde_json::from_str(&output.stdout_lines().join("\n"))?;
        assert_eq!(data.len(), 1);
        Ok(data.pop().unwrap())
    }

    fn run(
        &self,
        timeout: Option<Duration>,
        no_output_timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let res = Command::new(self.workspace, "docker")
            .args(&["start", "-a", &self.id])
            .timeout(timeout)
            .no_output_timeout(no_output_timeout)
            .run();
        let details = self.inspect()?;

        // Return a different error if the container was killed due to an OOM
        if details.state.oom_killed {
            if let Err(err) = res {
                Err(err.context(CommandError::SandboxOOM).into())
            } else {
                Err(CommandError::SandboxOOM.into())
            }
        } else {
            res
        }
    }

    fn delete(&self) -> Result<(), Error> {
        Command::new(self.workspace, "docker")
            .args(&["rm", "-f", &self.id])
            .run()
    }
}

/// Check whether the Docker daemon is running.
///
/// The Docker daemon is required for sandboxing to work, and this function returns whether the
/// daemon is online and reachable or not. Calling a sandboxed command when the daemon is offline
/// will error too, but this function allows the caller to error earlier.
pub fn docker_running(workspace: &Workspace) -> bool {
    info!("checking if the docker daemon is running");
    Command::new(workspace, "docker")
        .args(&["info"])
        .log_output(false)
        .run()
        .is_ok()
}
