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

    let mut args_ = vec!["run", "-i",
                         "--rm",
                         "-v", test_mount,
                         "-v", cargo_home_mount,
                         "-v", rustup_home_mount,
                         "-v", target_mount,
                         "-e", user_env,
                         "-e", cmd_env,
                         image_name];

    run::run("docker", &args_, &[])?;

    Ok(())
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
