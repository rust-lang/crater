use CARGO_HOME;
use RUSTUP_HOME;
use errors::*;
use run;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Builds the docker container image, 'cargobomb', what will be used
/// to isolate builds from each other. This expects the Dockerfile
/// to exist in the `docker` directory, at runtime.
pub fn build_container() -> Result<()> {
    run::run("docker", &["build", "-t", "cargobomb", "docker"], &[])
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

pub fn create_rust_container(env: &RustEnv) -> Result<Container> {
    log!("creating container for: {}", env.args.join(" "));

    fs::create_dir_all(&env.work_dir.0);
    fs::create_dir_all(&env.cargo_home.0);
    fs::create_dir_all(&env.rustup_home.0);
    fs::create_dir_all(&env.target_dir.0);

    let mount_arg = |host_path, container_path, perm| {
        let perm = match perm {
            Perm::ReadWrite => "rw",
            Perm::ReadOnly => "ro",
        };
        format!("{}:{}:{}",
                absolute(host_path).display(),
                container_path,
                perm)
    };

    let work_mount = mount_arg(&env.work_dir.0, "/source", env.work_dir.1);
    let cargo_home_mount = mount_arg(&env.cargo_home.0, "/cargo-home", env.cargo_home.1);
    let rustup_home_mount = mount_arg(&env.rustup_home.0, "/rustup-home", env.rustup_home.1);
    let target_mount = mount_arg(&env.target_dir.0, "/target", env.target_dir.1);

    let image_name = "cargobomb";

    let user_env = &format!("USER_ID={}", user_id());
    let cmd_env = &format!("CMD={}", env.args.join(" "));

    let docker_gid_;
    let mut args_ = vec![
        "-v",
        &work_mount,
        "-v",
        &cargo_home_mount,
        "-v",
        &rustup_home_mount,
        "-v",
        &target_mount,
        "-e",
        user_env,
        "-e",
        cmd_env,
    ];

    // Let the container talk to the docker daemon
    if env.privileged {
        let docker_socket_mount = "/var/run/docker.sock:/var/run/docker.sock";
        args_.push("-v");
        args_.push(docker_socket_mount);
        args_.push("-e");
        docker_gid_ = Some(format!("DOCKER_GROUP_ID={}", docker_gid()));
        args_.push(docker_gid_.as_ref().expect(""));
    }

    args_.push(image_name);

    create_container(&args_)
}

pub fn run(source_path: &Path, target_path: &Path, args: &[&str]) -> Result<()> {

    log!("running: {}", args.join(" "));

    let env = RustEnv {
        args: args,
        privileged: false,
        work_dir: (source_path.into(), Perm::ReadOnly),
        cargo_home: (Path::new(CARGO_HOME).into(), Perm::ReadOnly),
        rustup_home: (Path::new(RUSTUP_HOME).into(), Perm::ReadOnly),
        // This is configured as CARGO_TARGET_DIR by the docker container itself
        target_dir: (target_path.into(), Perm::ReadWrite),
    };

    let c = create_rust_container(&env)?;
    defer!{{
        delete_container(&c);
    }}
    run_container(&c)
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Container(String);

use std::fmt::{self, Display, Formatter};

impl Display for Container {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        self.0.fmt(f)
    }
}

fn create_container(args: &[&str]) -> Result<Container> {
    let mut args_ = vec!["create"];
    args_.extend(args.iter());
    let (out, _) = run::run_capture(None, "docker", &args_, &[])?;
    Ok(Container(out[0].clone()))
}

pub fn run_container(c: &Container) -> Result<()> {
    run::run("docker", &["start", "-a", &c.0], &[])
}

pub fn delete_container(c: &Container) -> Result<()> {
    run::run("docker", &["rm", "-f", &c.0], &[])
}
