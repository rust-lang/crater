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
    RunCommand::new(
        "docker",
        &["build", "-f", &dockerfile, "-t", IMAGE_NAME, "docker"],
    ).enable_timeout(false)
    .run()
}

#[derive(Copy, Clone)]
pub enum MountPerms {
    ReadWrite,
    ReadOnly,
}

struct MountConfig<'a> {
    host_path: PathBuf,
    container_path: &'a str,
    perm: MountPerms,
}

impl<'a> MountConfig<'a> {
    fn to_arg(&self) -> String {
        let perm = match self.perm {
            MountPerms::ReadWrite => "rw",
            MountPerms::ReadOnly => "ro",
        };
        format!(
            "{}:{}:{},Z",
            absolute(&self.host_path).display(),
            self.container_path,
            perm
        )
    }
}

pub struct ContainerBuilder<'a> {
    image: &'a str,
    mounts: Vec<MountConfig<'a>>,
    env: Vec<(&'static str, String)>,
    memory_limit: Option<Size>,
    networking_disabled: bool
}

impl<'a> ContainerBuilder<'a> {
    pub fn new(image: &'a str) -> Self {
        ContainerBuilder {
            image,
            mounts: Vec::new(),
            env: Vec::new(),
            memory_limit: None,
            networking_disabled: false
        }
    }

    pub fn mount(mut self, host_path: PathBuf, container_path: &'a str, perm: MountPerms) -> Self {
        self.mounts.push(MountConfig {
            host_path,
            container_path,
            perm,
        });
        self
    }

    pub fn env(mut self, key: &'static str, value: String) -> Self {
        self.env.push((key, value));
        self
    }

    pub fn memory_limit(mut self, limit: Size) -> Self {
        self.memory_limit = Some(limit);
        self
    }

    pub fn disable_networking(mut self) -> Self {
        self.networking_disabled = true;
        self
    }

    pub fn create(self) -> Result<Container> {
        let mut args: Vec<String> = vec!["create".into()];

        for mount in &self.mounts {
            fs::create_dir_all(&mount.host_path)?;
            args.push("-v".into());
            args.push(mount.to_arg())
        }

        for &(var, ref value) in &self.env {
            args.push("-e".into());
            args.push(format!{"{}={}", var, value})
        }

        if let Some(limit) = self.memory_limit {
            args.push("-m".into());
            args.push(limit.to_string());
        }

        if self.networking_disabled {
            args.push("--network".into());
            args.push("none".into());
        }

        args.push(self.image.into());

        let (out, _) = RunCommand::new("docker", &*args).run_capture()?;
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
        RunCommand::new("docker", &["start", "-a", &self.id])
            .quiet(quiet)
            .run()
    }

    pub fn delete(&self) -> Result<()> {
        RunCommand::new("docker", &["rm", "-f", &self.id]).run()
    }
}
