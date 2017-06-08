use dirs::{CARGO_HOME, RUSTUP_HOME, TARGET_DIR, TOOLCHAIN_DIR};
use dl;
use docker;
use errors::*;
use reqwest;
use run;
use std::env::consts::EXE_SUFFIX;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tempdir::TempDir;
use util;

const RUSTUP_BASE_URL: &'static str = "https://static.rust-lang.org/rustup/dist";

const RUST_CI_TRY_BASE_URL: &'static str = "https://rust-lang-ci.s3.amazonaws.com/rustc-builds-try";
const RUST_CI_MASTER_BASE_URL: &'static str = "https://rust-lang-ci.s3.amazonaws.com/rustc-builds/";

const RUST_CI_COMPONENTS: [(&'static str, &'static str); 3] =
    [
        ("rustc", "rustc-nightly-x86_64-unknown-linux-gnu.tar.gz"),
        ("rust-std-x86_64-unknown-linux-gnu", "rust-std-nightly-x86_64-unknown-linux-gnu.tar.gz"),
        ("cargo", "cargo-nightly-x86_64-unknown-linux-gnu.tar.gz"),
    ];

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
/// A toolchain name, either a rustup channel identifier,
/// or a URL+branch+sha: https://github.com/rust-lang/rust+master+sha
pub enum Toolchain {
    Dist(String), // rustup toolchain spec
    TryBuild { sha: String },
    Master { sha: String },
}

impl Toolchain {
    pub fn prepare(&self) -> Result<()> {
        init_rustup()?;

        match *self {
            Toolchain::Dist(ref toolchain) => init_toolchain_from_dist(toolchain)?,
            Toolchain::Master { ref sha } => init_toolchain_from_ci(RUST_CI_MASTER_BASE_URL, sha)?,
            Toolchain::TryBuild { ref sha } => init_toolchain_from_ci(RUST_CI_TRY_BASE_URL, sha)?,
        }

        Ok(())
    }
}

impl ToString for Toolchain {
    fn to_string(&self) -> String {
        match *self {
            Toolchain::Dist(ref s) => s.clone(),
            Toolchain::TryBuild { ref sha } => format!("try#{}", sha),
            Toolchain::Master { ref sha } => format!("master#{}", sha),
        }
    }
}

impl FromStr for Toolchain {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        fn get_sha(s: &str) -> Result<&str> {
            if let Some(hash_idx) = s.find('#') {
                let (_, sha) = s.split_at(hash_idx + 1);
                Ok(sha)
            } else {
                Err("no sha for try toolchain".into())
            }
        }
        if s.starts_with("try#") {
            Ok(Toolchain::TryBuild { sha: get_sha(s)?.into() })
        } else if s.starts_with("master#") {
            Ok(Toolchain::Master { sha: get_sha(s)?.into() })
        } else {
            Ok(Toolchain::Dist(s.to_string()))
        }
    }
}

fn init_rustup() -> Result<()> {
    fs::create_dir_all(*CARGO_HOME)?;
    fs::create_dir_all(*RUSTUP_HOME)?;
    if rustup_exists() {
        update_rustup()?;
    } else {
        install_rustup()?;
    }

    Ok(())
}

fn rustup_exe() -> String {
    format!("{}/bin/rustup{}", *CARGO_HOME, EXE_SUFFIX)
}

fn rustup_exists() -> bool {
    Path::new(&rustup_exe()).exists()
}

fn rustup_run(name: &str, args: &[&str]) -> Result<()> {
    let full_env = [("CARGO_HOME", *CARGO_HOME), ("RUSTUP_HOME", *RUSTUP_HOME)];
    run::run(name, args, &full_env)
}

fn install_rustup() -> Result<()> {
    info!("installing rustup");
    let rustup_url = &format!("{}/{}/rustup-init{}",
            RUSTUP_BASE_URL,
            &util::this_target(),
            EXE_SUFFIX);
    let mut response = dl::download(rustup_url)
        .chain_err(|| "unable to download rustup")?;

    let tempdir = TempDir::new("cargobomb")?;
    let installer = &tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
    {
        let mut file = File::create(installer)?;
        io::copy(&mut response, &mut file)?;
        make_executable(installer)?;
    }

    // FIXME: Wish I could install rustup without installing a toolchain
    util::try_hard(|| {
                       rustup_run(&installer.to_string_lossy(), &["-y", "--no-modify-path"])
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
    info!("updating rustup");
    util::try_hard(|| {
                       rustup_run(&rustup_exe(), &["self", "update"])
                           .chain_err(|| "unable to run rustup self-update")
                   })
}

fn init_toolchain_from_dist(toolchain: &str) -> Result<()> {
    info!("installing toolchain {}", toolchain);
    util::try_hard(|| {
                       rustup_run(&rustup_exe(), &["toolchain", "install", toolchain])
                           .chain_err(|| "unable to install toolchain via rustup")
                   })
}

fn init_toolchain_from_ci(base_url: &str, sha: &str) -> Result<()> {
    info!("installing toolchain try#{}", sha);

    fs::create_dir_all(&*TOOLCHAIN_DIR)?;
    let dir = TOOLCHAIN_DIR.join(sha);
    use rustup_dist as dist;
    use rustup_dist::component::Package;

    let prefix = dist::prefix::InstallPrefix::from(dir);
    let target = dist::component::Components::open(prefix.clone())?;
    let cfg = dist::temp::Cfg::new((*RUSTUP_HOME).into(), RUSTUP_BASE_URL, Box::new(|_| {}));
    let notifier = |_: dist::notifications::Notification| {};
    let mut tx = dist::component::Transaction::new(prefix, &cfg, &notifier);

    for &(component, file) in &RUST_CI_COMPONENTS {
        if target.find(component)?.is_some() {
            info!("skipping component {}, already installed", component);
            continue;
        };
        info!("installing component {}", component);
        let url = format!("{}/{}/{}", base_url, sha, file);
        let response = dl::download_limit(&url, 10000)?;
        if *response.status() != reqwest::StatusCode::Ok {
            return Err(ErrorKind::Download.into());
        }
        tx = dist::component::TarGzPackage::new(response, &cfg)?
            .install(&target, component, None, tx)?;
    }
    tx.commit();

    Ok(())
}

impl Toolchain {
    pub fn rustup_name(&self) -> String {
        match *self {
            Toolchain::Dist(ref n) => n.to_string(),
            Toolchain::TryBuild { ref sha } |
            Toolchain::Master { ref sha } => sha.to_string(),
        }
    }
}

pub fn ex_target_dir(ex_name: &str) -> PathBuf {
    TARGET_DIR.join(ex_name)
}

pub enum CargoState {
    Locked,
    Unlocked,
}

impl Toolchain {
    pub fn target_dir(&self, ex_name: &str) -> PathBuf {
        ex_target_dir(ex_name).join(self.to_string())
    }

    pub fn run_cargo(&self,
                     ex_name: &str,
                     source_dir: &Path,
                     args: &[&str],
                     cargo_state: CargoState)
                     -> Result<()> {
        let toolchain_name = self.rustup_name();
        let ex_target_dir = self.target_dir(ex_name);

        fs::create_dir_all(&ex_target_dir)?;

        let toolchain_arg = "+".to_string() + &toolchain_name;
        let mut full_args = vec!["cargo", &*toolchain_arg];
        full_args.extend_from_slice(args);

        info!("running: {}", full_args.join(" "));
        let perm = match cargo_state {
            CargoState::Locked => docker::Perm::ReadOnly,
            CargoState::Unlocked => docker::Perm::ReadWrite,
        };
        let rust_env = docker::RustEnv {
            args: &full_args,
            work_dir: (source_dir.into(), perm),
            cargo_home: (Path::new(*CARGO_HOME).into(), perm),
            rustup_home: (Path::new(*RUSTUP_HOME).into(), docker::Perm::ReadOnly),
            // This is configured as CARGO_TARGET_DIR by the docker container itself
            target_dir: (ex_target_dir, docker::Perm::ReadWrite),
        };
        docker::run(docker::rust_container(rust_env))
    }
}
