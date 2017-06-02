use dirs::{CARGO_HOME, RUSTUP_HOME};
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
    pub privileged: bool,
    pub work_dir: (PathBuf, Perm),
    pub cargo_home: (PathBuf, Perm),
    pub rustup_home: (PathBuf, Perm),
    pub target_dir: (PathBuf, Perm),
}

pub struct MountConfig<'a> {
    pub host_path: &'a Path,
    pub container_path: &'a str,
    pub perm: Perm,
}

impl<'a> MountConfig<'a> {
    fn as_arg(&self) -> String {
        let perm = match self.perm {
            Perm::ReadWrite => "rw",
            Perm::ReadOnly => "ro",
        };
        format!("{}:{}:{}",
                absolute(self.host_path).display(),
                self.container_path,
                perm)
    }
}

pub struct ContainerConfig<'a> {
    pub image_name: &'a str,
    pub mounts: &'a [MountConfig<'a>],
    pub env: &'a [(&'static str, String)],
}


pub fn run(source_path: &Path, target_path: &Path, args: &[&str]) -> Result<()> {

    info!("running: {}", args.join(" "));

    let env = RustEnv {
        args: args,
        privileged: false,
        work_dir: (source_path.into(), Perm::ReadOnly),
        cargo_home: (Path::new(CARGO_HOME).into(), Perm::ReadOnly),
        rustup_home: (Path::new(RUSTUP_HOME).into(), Perm::ReadOnly),
        // This is configured as CARGO_TARGET_DIR by the docker container itself
        target_dir: (target_path.into(), Perm::ReadWrite),
    };

    let c = Container::create_rust_container(&env)?;
    defer!{{
        if let Err(e) = c.delete() {
            error!{"Cannot delete container: {}", e; "container" => &c.id}
        }
    }}
    c.run()
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

#[cfg(unix)]
fn docker_gid() -> ::libc::gid_t {
    unsafe {
        use std::ffi::CString;
        use libc::{c_char, c_int, group, size_t};
        use std::mem;
        use std::ptr;
        use std::iter;

        extern "C" {
            fn getgrnam_r(name: *const c_char,
                          grp: *mut group,
                          buf: *mut c_char,
                          buflen: size_t,
                          result: *mut *mut group)
                          -> c_int;
        }

        let name = CString::new("docker").expect("");
        // FIXME don't guess bufer size
        let mut buf = iter::repeat(0 as c_char).take(1024).collect::<Vec<_>>();
        let mut group = mem::uninitialized();
        let mut ptr = ptr::null_mut();
        let r = getgrnam_r(name.as_ptr(),
                           &mut group,
                           buf.as_mut_ptr(),
                           buf.len(),
                           &mut ptr);
        if r != 0 {
            panic!("getgrnam_r failed retrieving docker gid");
        }

        group.gr_gid
    }
}

#[cfg(windows)]
fn docker_gid() -> u32 {
    panic!("unimplemented docker_gid");
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
    pub fn create_rust_container(config: &RustEnv) -> Result<Self> {
        info!("creating container for: {}", config.args.join(" "));

        fs::create_dir_all(&config.work_dir.0)?;
        fs::create_dir_all(&config.cargo_home.0)?;
        fs::create_dir_all(&config.rustup_home.0)?;
        fs::create_dir_all(&config.target_dir.0)?;

        let docker_gid_;

        let mut mounts = vec![
            MountConfig {
                host_path: &config.work_dir.0,
                container_path: "/source",
                perm: config.work_dir.1,
            },
            MountConfig {
                host_path: &config.target_dir.0,
                container_path: "/target",
                perm: config.target_dir.1,
            },
            MountConfig {
                host_path: &config.cargo_home.0,
                container_path: "/cargo-home",
                perm: config.cargo_home.1,
            },
            MountConfig {
                host_path: &config.rustup_home.0,
                container_path: "/rustup-home",
                perm: config.rustup_home.1,
            },
        ];

        let mut env = vec![
            ("USER_ID", format!("{}", user_id())),
            ("CMD", config.args.join(" ")),
        ];


        // Let the container talk to the docker daemon
        if config.privileged {
            mounts.push(MountConfig {
                            host_path: Path::new("/var/run/docker.sock"),
                            container_path: "/var/run/docker.sock",
                            perm: Perm::ReadWrite,
                        });
            docker_gid_ = format!("DOCKER_GROUP_ID={}", docker_gid());
            env.push(("DOCKER_GROUP_ID", docker_gid_));
        }

        Self::create_container(&ContainerConfig {
                                   image_name: IMAGE_NAME,
                                   mounts: &*mounts,
                                   env: &env,
                               })
    }

    fn create_container(config: &ContainerConfig) -> Result<Self> {
        let mut args: Vec<String> = vec!["create".into()];

        for mount in config.mounts {
            args.push("-v".into());
            args.push(mount.as_arg())
        }

        for &(var, ref value) in config.env {
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
