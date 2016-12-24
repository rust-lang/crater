use errors::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::env;
use run;
use CARGO_HOME;
use RUSTUP_HOME;

/// Builds the docker container image, 'cargobomb', what will be used
/// to isolate builds from each other. This expects the Dockerfile
/// to exist in the `docker` directory, at runtime.
pub fn build_container() -> Result<()> {
    run::run("docker", &["build", "-t", "cargobomb", "docker"], &[])
}

#[derive(Copy, Clone)]
pub enum Perm { ReadWrite, ReadOnly }

pub struct RustEnv<'a> {
    pub args: &'a [&'a str],
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
                container_path, perm)
    };

    let work_mount = mount_arg(&env.work_dir.0, "/source", env.work_dir.1);
    let cargo_home_mount = mount_arg(&env.cargo_home.0, "/cargo-home", env.cargo_home.1);
    let rustup_home_mount = mount_arg(&env.rustup_home.0, "/rustup-home", env.rustup_home.1);
    let target_mount = mount_arg(&env.target_dir.0, "/target", env.target_dir.1);

    let image_name = "cargobomb";

    let user_env = &format!("USER_ID={}", user_id());
    let cmd_env = &format!("CMD={}", env.args.join(" "));

    let ref args_ = vec!["-v", &work_mount,
                         "-v", &cargo_home_mount,
                         "-v", &rustup_home_mount,
                         "-v", &target_mount,
                         "-e", &user_env,
                         "-e", &cmd_env,
                         image_name];

    create_container(args_)
}

pub fn run(source_path: &Path, target_path: &Path, args: &[&str]) -> Result<()> {

    log!("running: {}", args.join(" "));

    let source_dir=absolute(source_path);
    let cargo_home=absolute(Path::new(CARGO_HOME));
    let rustup_home=absolute(Path::new(RUSTUP_HOME));
    // This is configured as CARGO_TARGET_DIR by the docker container itself
    let target_dir=absolute(target_path);

    fs::create_dir_all(&source_dir);
    fs::create_dir_all(&cargo_home);
    fs::create_dir_all(&rustup_home);
    fs::create_dir_all(&target_dir);

    let test_mount = &format!("{}:/source:ro", source_dir.display());
    let cargo_home_mount = &format!("{}:/cargo-home:ro", cargo_home.display());
    let rustup_home_mount = &format!("{}:/rustup-home:ro", rustup_home.display());
    let target_mount = &format!("{}:/target", target_dir.display());

    let image_name = "cargobomb";

    let user_env = &format!("USER_ID={}", user_id());
    let cmd_env = &format!("CMD={}", args.join(" "));

    let ref args_ = vec!["-v", test_mount,
                         "-v", cargo_home_mount,
                         "-v", rustup_home_mount,
                         "-v", target_mount,
                         "-e", user_env,
                         "-e", cmd_env,
                         image_name];

    run_in_docker(args_)
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

fn run_in_docker(args: &[&str]) -> Result<()> {
    let ref c = create_container(args)?;
    run_container(c)?;
    Ok(())
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
    let mut args_ = vec![
        "create"
    ];
    args_.extend(args.iter());
    let (out, _) = run::run_capture(None, "docker", &args_, &[])?;
    Ok(Container(out[0].clone()))
}

fn run_container(c: &Container) -> Result<()> {
    defer!{{
        delete_container(c);
    }}
    run::run("docker", &["start", "-a", &c.0], &[])
}

pub fn start_container(c: &Container) -> Result<()> {
    run::run("docker", &["start", &c.0], &[])
}

pub fn wait_for_container(c: &Container) -> Result<()> {
    run::run("docker", &["wait", &c.0], &[])
}

pub fn delete_container(c: &Container) -> Result<()> {
    run::run("docker", &["rm", "-f", &c.0], &[])
}
