use errors::*;
use run::RunCommand;
use std::env;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use utils::size::Size;

pub static IMAGE_NAME: &'static str = "crater";

/// Builds the docker container image, 'crater', what will be used
/// to isolate builds from each other. This expects the Dockerfile
/// to exist in the `docker` directory, at runtime.
pub fn build_container(docker_env: &str) -> Result<()> {
    let dockerfile = format!("docker/Dockerfile.{}", docker_env);
    RunCommand::new("docker")
        .args(&["build", "-f", &dockerfile, "-t", IMAGE_NAME, "docker"])
        .enable_timeout(false)
        .run()
}

pub(crate) fn is_running() -> bool {
    RunCommand::new("docker").args(&["info"]).run().is_ok()
}

#[derive(Copy, Clone)]
pub enum MountPerms {
    ReadWrite,
    ReadOnly,
}

struct MountConfig {
    host_path: PathBuf,
    container_path: PathBuf,
    perm: MountPerms,
}

impl MountConfig {
    fn to_arg(&self) -> String {
        let perm = match self.perm {
            MountPerms::ReadWrite => "rw",
            MountPerms::ReadOnly => "ro",
        };
        format!(
            "{}:{}:{},Z",
            absolute(&self.host_path).to_string_lossy(),
            self.container_path.to_string_lossy(),
            perm
        )
    }
}

pub struct ContainerBuilder {
    image: String,
    mounts: Vec<MountConfig>,
    env: Vec<(String, String)>,
    memory_limit: Option<Size>,
    enable_networking: bool,
}

impl ContainerBuilder {
    pub fn new<S: Into<String>>(image: S) -> Self {
        ContainerBuilder {
            image: image.into(),
            mounts: Vec::new(),
            env: Vec::new(),
            memory_limit: None,
            enable_networking: true,
        }
    }

    pub fn mount<P1: Into<PathBuf>, P2: Into<PathBuf>>(
        mut self,
        host_path: P1,
        container_path: P2,
        perm: MountPerms,
    ) -> Self {
        self.mounts.push(MountConfig {
            host_path: host_path.into(),
            container_path: container_path.into(),
            perm,
        });
        self
    }

    pub fn env<S1: Into<String>, S2: Into<String>>(mut self, key: S1, value: S2) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn memory_limit(mut self, limit: Option<Size>) -> Self {
        self.memory_limit = limit;
        self
    }

    pub fn enable_networking(mut self, enable: bool) -> Self {
        self.enable_networking = enable;
        self
    }

    pub fn create(self) -> Result<Container> {
        let mut args: Vec<String> = vec!["create".into()];

        for mount in &self.mounts {
            fs::create_dir_all(&mount.host_path)?;
            args.push("-v".into());
            args.push(mount.to_arg())
        }

        for &(ref var, ref value) in &self.env {
            args.push("-e".into());
            args.push(format!{"{}={}", var, value})
        }

        if let Some(limit) = self.memory_limit {
            args.push("-m".into());
            args.push(limit.to_string());
        }

        if !self.enable_networking {
            args.push("--network".into());
            args.push("none".into());
        }

        args.push(self.image);

        let (out, _) = RunCommand::new("docker").args(&*args).run_capture()?;
        Ok(Container { id: out[0].clone() })
    }

    pub fn run(self, quiet: bool) -> Result<()> {
        let container = self.create()?;

        // Ensure the container is properly deleted even if something panics
        defer! {{
            if let Err(err) = container.delete().chain_err(|| format!("failed to delete container {}", container.id)) {
                ::utils::report_error(&err);
            }
        }}

        container.run(quiet)?;
        Ok(())
    }
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        let cd = env::current_dir().expect("unable to get current dir");
        cd.join(path)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Container {
    // Docker container ID
    id: String,
}

impl Display for Container {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

impl Container {
    pub fn run(&self, quiet: bool) -> Result<()> {
        RunCommand::new("docker")
            .args(&["start", "-a", &self.id])
            .quiet(quiet)
            .run()
    }

    pub fn delete(&self) -> Result<()> {
        RunCommand::new("docker")
            .args(&["rm", "-f", &self.id])
            .run()
    }
}
