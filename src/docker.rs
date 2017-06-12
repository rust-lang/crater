use errors::*;
use run;
use std::env;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

static IMAGE_NAME: &'static str = "cargobomb";

/// Builds the docker container image, 'cargobomb', what will be used
/// to isolate builds from each other. This expects the Dockerfile
/// to exist in the `docker` directory, at runtime.
pub fn build_container() -> Result<()> {
    run::run("docker", &["build", "-t", IMAGE_NAME, "docker"], &[])
}

#[derive(Copy, Clone)]
pub enum Perm {
    ReadWrite,
    ReadOnly,
}

pub struct RustEnv<'a> {
    pub args: &'a [&'a str],
    pub work_dir: (PathBuf, Perm),
    pub cargo_home: (PathBuf, Perm),
    pub rustup_home: (PathBuf, Perm),
    pub target_dir: (PathBuf, Perm),
}

pub struct MountConfig<'a> {
    pub host_path: PathBuf,
    pub container_path: &'a str,
    pub perm: Perm,
}

impl<'a> MountConfig<'a> {
    fn to_arg(&self) -> String {
        let perm = match self.perm {
            Perm::ReadWrite => "rw",
            Perm::ReadOnly => "ro",
        };
        format!(
            "{}:{}:{},Z",
            absolute(&self.host_path).display(),
            self.container_path,
            perm
        )
    }
}

pub struct ContainerConfig<'a> {
    pub image_name: &'a str,
    pub mounts: Vec<MountConfig<'a>>,
    pub env: Vec<(&'static str, String)>,
}


pub fn run(config: ContainerConfig) -> Result<()> {
    let c = Container::create_container(config)?;
    defer!{{
        if let Err(e) = c.delete() {
            error!{"Cannot delete container: {}", e; "container" => &c.id}
        }
    }}
    c.run()
}

pub fn rust_container(config: RustEnv) -> ContainerConfig {
    info!("creating container for: {}", config.args.join(" "));

    let mounts = vec![
        MountConfig {
            host_path: config.work_dir.0,
            container_path: "/source",
            perm: config.work_dir.1,
        },
        MountConfig {
            host_path: config.target_dir.0,
            container_path: "/target",
            perm: config.target_dir.1,
        },
        MountConfig {
            host_path: config.cargo_home.0,
            container_path: "/cargo-home",
            perm: config.cargo_home.1,
        },
        MountConfig {
            host_path: config.rustup_home.0,
            container_path: "/rustup-home",
            perm: config.rustup_home.1,
        },
    ];

    let env = vec![
        ("USER_ID", format!("{}", user_id())),
        ("CMD", config.args.join(" ")),
    ];

    ContainerConfig {
        image_name: IMAGE_NAME,
        mounts: mounts,
        env: env,
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

#[cfg(unix)]
fn user_id() -> ::libc::uid_t {
    unsafe { ::libc::geteuid() }
}

#[cfg(windows)]
fn user_id() -> u32 {
    panic!("unimplemented user_id");
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
    fn create_container(config: ContainerConfig) -> Result<Self> {
        let mut args: Vec<String> = vec!["create".into()];

        for mount in &config.mounts {
            fs::create_dir_all(&mount.host_path)?;
            args.push("-v".into());
            args.push(mount.to_arg())
        }

        for &(var, ref value) in &config.env {
            args.push("-e".into());
            args.push(format!{"{}={}", var, value})
        }
        args.push(config.image_name.into());

        let (out, _) = run::run_capture(None, "docker", &*args, &[])?;
        Ok(Self { id: out[0].clone() })
    }

    pub fn run(&self) -> Result<()> {
        run::run("docker", &["start", "-a", &self.id], &[])
    }

    pub fn delete(&self) -> Result<()> {
        run::run("docker", &["rm", "-f", &self.id], &[])
    }
}
