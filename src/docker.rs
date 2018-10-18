use prelude::*;
use regex::Regex;
use run::RunCommand;
use std::env;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use utils::size::Size;

lazy_static! {
    static ref LOCAL_ENV_REGEX: Regex = Regex::new(r"^(?P<image>[a-zA-Z0-9_-]+)@local$").unwrap();
    static ref REMOTE_ENV_REGEX: Regex =
        Regex::new(r"^(?P<org>[a-zA-Z0-9]+/)?(?P<image>[a-zA-Z0-9_-]+)$").unwrap();
}

static DEFAULT_IMAGES_ORG: &str = "rustops";
static DEFAULT_IMAGES_PREFIX: &str = "crater-env";

#[derive(Debug, Fail)]
pub(crate) enum DockerError {
    #[fail(display = "container ran out of memory")]
    ContainerOOM,
    #[fail(display = "missing docker image: {}", _0)]
    MissingImage(String),
    #[fail(display = "invalid docker environment: {}", _0)]
    InvalidEnvironment(String),
}

pub(crate) struct DockerEnv {
    image: String,
    local: bool,
}

impl DockerEnv {
    pub(crate) fn new(env: &str) -> Fallible<DockerEnv> {
        let env = Self::parse(env)?;

        // If the image has a remote, try to pull it
        if !env.local {
            info!("updating the docker image {}", env.image);
            RunCommand::new("docker")
                .args(&["pull", &env.image])
                .run()?;
        }

        // Check if the image exists locally
        info!("checking if the docker image {} exists locally", env.image);
        let image_exists = RunCommand::new("docker")
            .args(&["image", "inspect", &env.image, "-f", "ok"])
            .run()
            .is_ok();

        if image_exists {
            Ok(env)
        } else {
            Err(DockerError::MissingImage(env.image.clone()).into())
        }
    }

    fn parse(input: &str) -> Fallible<DockerEnv> {
        if let Some(captures) = LOCAL_ENV_REGEX.captures(input) {
            Ok(DockerEnv {
                image: format!("{}-{}", DEFAULT_IMAGES_PREFIX, captures[1].to_string()),
                local: true,
            })
        } else if let Some(captures) = REMOTE_ENV_REGEX.captures(input) {
            if captures.name("org").is_some() {
                Ok(DockerEnv {
                    image: input.to_string(),
                    local: false,
                })
            } else {
                Ok(DockerEnv {
                    image: format!(
                        "{}/{}-{}",
                        DEFAULT_IMAGES_ORG, DEFAULT_IMAGES_PREFIX, &captures["image"]
                    ),
                    local: false,
                })
            }
        } else {
            Err(DockerError::InvalidEnvironment(input.to_string()).into())
        }
    }
}

pub(crate) fn is_running() -> bool {
    RunCommand::new("docker").args(&["info"]).run().is_ok()
}

#[derive(Copy, Clone)]
pub(crate) enum MountPerms {
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

pub(crate) struct ContainerBuilder {
    image: String,
    mounts: Vec<MountConfig>,
    env: Vec<(String, String)>,
    memory_limit: Option<Size>,
    enable_networking: bool,
}

impl ContainerBuilder {
    pub(crate) fn new(docker_env: &DockerEnv) -> Self {
        ContainerBuilder {
            image: docker_env.image.clone(),
            mounts: Vec::new(),
            env: Vec::new(),
            memory_limit: None,
            enable_networking: true,
        }
    }

    pub(crate) fn mount<P1: Into<PathBuf>, P2: Into<PathBuf>>(
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

    pub(crate) fn env<S1: Into<String>, S2: Into<String>>(mut self, key: S1, value: S2) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub(crate) fn memory_limit(mut self, limit: Option<Size>) -> Self {
        self.memory_limit = limit;
        self
    }

    pub(crate) fn enable_networking(mut self, enable: bool) -> Self {
        self.enable_networking = enable;
        self
    }

    pub(crate) fn create(self) -> Fallible<Container> {
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

    pub(crate) fn run(self, quiet: bool) -> Fallible<()> {
        let container = self.create()?;

        // Ensure the container is properly deleted even if something panics
        defer! {{
            if let Err(err) = container.delete().with_context(|_| format!("failed to delete container {}", container.id)) {
                ::utils::report_failure(&err);
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Container {
    // Docker container ID
    id: String,
}

impl Display for Container {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

impl Container {
    fn inspect(&self) -> Fallible<InspectContainer> {
        let output = RunCommand::new("docker")
            .args(&["inspect", &self.id])
            .hide_output(true)
            .run_capture()?;

        let mut data: Vec<InspectContainer> = ::serde_json::from_str(&output.0.join("\n"))?;
        assert_eq!(data.len(), 1);
        Ok(data.pop().unwrap())
    }

    pub(crate) fn run(&self, quiet: bool) -> Fallible<()> {
        let res = RunCommand::new("docker")
            .args(&["start", "-a", &self.id])
            .quiet(quiet)
            .run();
        let details = self.inspect()?;

        // Return a different error if the container was killed due to an OOM
        if details.state.oom_killed {
            if let Err(err) = res {
                Err(err.context(DockerError::ContainerOOM).into())
            } else {
                Err(DockerError::ContainerOOM.into())
            }
        } else {
            res
        }
    }

    pub(crate) fn delete(&self) -> Fallible<()> {
        RunCommand::new("docker")
            .args(&["rm", "-f", &self.id])
            .run()
    }
}

#[cfg(test)]
mod tests {
    use super::DockerEnv;

    #[test]
    fn test_docker_env_parsing() {
        let local = DockerEnv::parse("foo@local").unwrap();
        assert_eq!(local.image.as_str(), "foo");
        assert!(custom.local);

        let rustops = DockerEnv::parse("mini").unwrap();
        assert_eq!(rustops.image.as_str(), "rustops/crater-env-mini");
        assert!(!custom.local);

        let custom = DockerEnv::parse("foo/bar").unwrap();
        assert_eq!(custom.image.as_str(), "foo/bar");
        assert!(!custom.local);

        for invalid in &[
            "foo/bar@local",
            "foo/bar/baz",
            "foo/bar:baz",
            "foo:bar",
            "foo:bar@local",
        ] {
            assert!(DockerEnv::parse(invalid).is_err());
        }
    }
}
