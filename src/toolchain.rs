use tempdir::TempDir;
use errors::*;
use std::env::consts::EXE_SUFFIX;
use url::Url;
use std::path::{Path, PathBuf};
use CARGO_HOME;
use RUSTUP_HOME;
use std::fs::{self, File};
use std::cell::RefCell;
use std::io::Write;
use dl;
use util;
use log;
use run;
use TOOLCHAIN_DIR;
use git;
use TARGET_DIR;
use toolchain;

const RUSTUP_BASE_URL: &'static str = "https://static.rust-lang.org/rustup/dist";

#[derive(Serialize, Deserialize, PartialEq)]
pub enum Toolchain {
    Dist(String), // rustup toolchain spec
    Repo(String, String), // Url, Sha
}

pub fn prepare_toolchain(toolchain: &str) -> Result<()> {
    let toolchain = parse_toolchain(toolchain)?;
    prepare_toolchain_(&toolchain)
}

pub fn prepare_toolchain_(toolchain: &Toolchain) -> Result<()> {
    init_rustup()?;

    match *toolchain {
        Toolchain::Dist(ref toolchain) => init_toolchain_from_dist(toolchain)?,
        Toolchain::Repo(ref repo, ref sha) => init_toolchain_from_repo(repo, sha)?,
    }

    Ok(())
}

pub fn parse_toolchain(toolchain: &str) -> Result<Toolchain> {
    if toolchain.starts_with("https://") {
        if let Some(hash_idx) = toolchain.find('#') {
            let repo = &toolchain[.. hash_idx];
            let sha = &toolchain[hash_idx + 1 ..];
            Ok(Toolchain::Repo(repo.to_string(), sha.to_string()))
        } else {
            Err("no sha for git toolchain".into())
        }
    } else {
        Ok(Toolchain::Dist(toolchain.to_string()))
    }
}

pub fn tc_to_string(tc: &Toolchain) -> String {
    match *tc {
        Toolchain::Dist(ref s) => s.clone(),
        Toolchain::Repo(ref url, ref sha) => format!("{}#{}", url, sha),
    }
}

fn init_rustup() -> Result<()> {
    fs::create_dir_all(CARGO_HOME)?;
    fs::create_dir_all(RUSTUP_HOME)?;
    if !rustup_exists() {
        install_rustup()?;
    } else {
        update_rustup()?;
    }

    Ok(())
}

fn rustup_exe() -> String {
    format!("{}/bin/rustup{}", CARGO_HOME, EXE_SUFFIX)
}

fn rustup_exists() -> bool {
    Path::new(&rustup_exe()).exists()
}

fn rustup_run(name: &str,
              args: &[&str],
              env: &[(&str, &str)]) -> Result<()> {
    let mut full_env = [("CARGO_HOME", CARGO_HOME),
                   ("RUSTUP_HOME", RUSTUP_HOME)].to_vec();
    full_env.extend(env.iter());
    run::run(name, args, &full_env)
}

fn install_rustup() -> Result<()> {
    log!("installing rustup");
    let ref rustup_url = format!("{}/{}/rustup-init{}",
                                 RUSTUP_BASE_URL, &util::this_target(), EXE_SUFFIX);
    let buf = dl::download(rustup_url).chain_err(|| "unable to download rustup")?;

    let tempdir = TempDir::new("cargobomb")?;
    let ref installer = tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
    {
        let mut file = File::create(installer)?;
        file.write_all(&buf)?;
        make_executable(installer);
    }

    // FIXME: Wish I could install rustup without installing a toolchain
    util::try_hard(|| {
        rustup_run(&installer.to_string_lossy(),
                   &["-y", "--no-modify-path"],
                   &[])
            .chain_err(|| "unable to run rustup-init")
    })
}

pub fn make_executable(path: &Path) -> Result<()> {
    #[cfg(windows)]
    fn inner(_: &Path) -> Result<()> {
        Ok(())
    }
    #[cfg(not(windows))]
    fn inner(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(path)?;

        let mut perms = metadata.permissions();
        let new_mode = (perms.mode() & !0o777) | 0o755;
        perms.set_mode(new_mode);

        fs::set_permissions(path, perms)?;

        Ok(())
    }

    inner(path)
}

fn update_rustup() -> Result<()> {
    log!("updating rustup");
    util::try_hard(|| {
        rustup_run(&rustup_exe(),
                   &["self", "update"],
                   &[])
            .chain_err(|| "unable to run rustup self-update")
    })
}

fn init_toolchain_from_dist(toolchain: &str) -> Result<()> {
    log!("installing toolchain {}", toolchain);
    util::try_hard(|| {
        rustup_run(&rustup_exe(),
                   &["toolchain", "install", toolchain],
                   &[])
            .chain_err(|| "unable to install toolchain via rustup")
    })
}

fn init_toolchain_from_repo(repo: &str, sha: &str) -> Result<()> {
    log!("installing toolchain {}#{}", repo, sha);

    fs::create_dir_all(TOOLCHAIN_DIR)?;
    let ref dir = Path::new(TOOLCHAIN_DIR).join(sha);
    git::shallow_clone_or_pull(repo, dir)?;
    git::shallow_fetch_sha(repo, dir, sha)?;
    git::reset_to_sha(dir, sha)?;

    panic!()
}

pub fn rustup_toolchain_name(toolchain: &str) -> Result<String> {
    let toolchain = parse_toolchain(toolchain)?;
    Ok(match toolchain {
        Toolchain::Dist(ref n) => n.to_string(),
        Toolchain::Repo(_, _) => {
            panic!()
        }
    })
}

pub fn ex_target_dir(ex_name: &str) -> PathBuf {
    Path::new(TARGET_DIR).join(ex_name)
}

pub fn target_dir(ex_name: &str, toolchain: &str) -> PathBuf {
    ex_target_dir(ex_name).join(toolchain)
}

pub fn run_cargo(toolchain: &str, ex_name: &str, args: &[&str]) -> Result<()> {
    let toolchain_name = rustup_toolchain_name(&toolchain)?;
    let ex_target_dir = target_dir(ex_name, toolchain);

    fs::create_dir_all(&ex_target_dir)?;

    let toolchain_arg = "+".to_string() + &toolchain_name;
    let ref mut full_args = [&*toolchain_arg].to_vec();
    full_args.extend_from_slice(args);

    let cargo = Path::new(CARGO_HOME).join("bin/cargo");
    rustup_run(&cargo.to_string_lossy(),
               full_args,
               &[("CARGO_TARGET_DIR", &ex_target_dir.to_string_lossy())])
}
