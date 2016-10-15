use tempdir::TempDir;
use errors::*;
use std::env::consts::EXE_SUFFIX;
use url::Url;
use std::path::{Path, PathBuf};
use CARGO_HOME;
use RUSTUP_HOME;
use std::fs::{self, File};
use std::cell::RefCell;
use std::process::Command;
use std::io::Write;
use dl;
use util;

const RUSTUP_BASE_URL: &'static str = "https://static.rust-lang.org/rustup/dist";

enum Toolchain {
    Dist(String), // rustup toolchain spec
    Repo(String, String), // Url, Sha
}

pub fn prepare_toolchain(toolchain: &str, target: &str) -> Result<()> {
    let toolchain = parse_toolchain(toolchain)?;
    init_rustup(target)?;

    match toolchain {
        Toolchain::Dist(toolchain) => init_toolchain_from_dist(&toolchain)?,
        Toolchain::Repo(repo, sha) => init_toolchain_from_repo(&repo, &sha)?,
    }

    Ok(())
}

fn parse_toolchain(toolchain: &str) -> Result<Toolchain> {
    if toolchain.starts_with("https://") {
        if let Some(hash_idx) = toolchain.find("#") {
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

fn init_rustup(target: &str) -> Result<()> {
    fs::create_dir_all(CARGO_HOME)?;
    fs::create_dir_all(RUSTUP_HOME)?;
    if !rustup_exists() {
        install_rustup(target)?;
    } else {
        update_rustup()?;
    }

    Ok(())
}

fn rustup_exe() -> PathBuf {
    PathBuf::from(format!("{}/bin/rustup{}", CARGO_HOME, EXE_SUFFIX))
}

fn rustup_exists() -> bool {
    Path::new(&rustup_exe()).exists()
}

fn install_rustup(target: &str) -> Result<()> {
    log!("installing rustup");
    let ref rustup_url = format!("{}/{}/rustup-init{}",
                                 RUSTUP_BASE_URL, target, EXE_SUFFIX);
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
        let status = command(installer)
            .arg("-y")
            .arg("--no-modify-path")
            .status()
            .chain_err(|| "unable to run rustup-init")?;

        if status.success() {
            Ok(())
        } else {
            Err("rustup installation failed".into())
        }
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

        let metadata = try!(fs::metadata(path).chain_err(|| {
            ErrorKind::SettingPermissions {
                path: PathBuf::from(path),
            }
        }));
        let mut perms = metadata.permissions();
        let new_mode = (perms.mode() & !0o777) | 0o755;
        perms.set_mode(new_mode);

        set_permissions(path, perms)
    }

    inner(path)
}

fn update_rustup() -> Result<()> {
    log!("updating rustup");
    util::try_hard(|| {
        let status = command(&rustup_exe())
            .arg("self").arg("update")
            .status()
            .chain_err(|| "unable to run rustup self-update")?;

        if status.success() {
            Ok(())
        } else {
            Err("rustup self-update failed".into())
        }
    })
}

fn command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    cmd.env("CARGO_HOME", CARGO_HOME)
        .env("RUSTUP_HOME", RUSTUP_HOME);
    cmd
}

fn init_toolchain_from_dist(toolchain: &str) -> Result<()> {
    log!("installing toolchain {}", toolchain);
    util::try_hard(|| {
        let status = command(&rustup_exe())
            .arg("toolchain")
            .arg("install")
            .arg(toolchain)
            .status()
            .chain_err(|| "unable to install toolchain via rustup")?;

        if status.success() {
            Ok(())
        } else {
            Err("toolchain installation failed".into())
        }
    })
}

fn init_toolchain_from_repo(repo: &str, sha: &str) -> Result<()> {
    log!("installing toolchain {}#{}", repo, sha);

    panic!()
}
