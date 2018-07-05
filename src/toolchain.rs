use config::Config;
use dirs::{CARGO_HOME, RUSTUP_HOME, TARGET_DIR};
use dl;
use docker::{ContainerBuilder, MountPerms, IMAGE_NAME};
use errors::*;
use ex::Experiment;
use run::RunCommand;
use std::env::consts::EXE_SUFFIX;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tempdir::TempDir;
use util;

const RUSTUP_BASE_URL: &str = "https://static.rust-lang.org/rustup/dist";

pub fn ex_target_dir(ex_name: &str) -> PathBuf {
    TARGET_DIR.join(ex_name)
}

#[derive(Copy, Clone)]
pub enum CargoState {
    Locked,
    Unlocked,
}

/// A toolchain name, either a rustup channel identifier,
/// or a URL+branch+sha: `https://github.com/rust-lang/rust+master+sha`
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
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
            Toolchain::Master { ref sha } | Toolchain::TryBuild { ref sha } => {
                init_toolchain_from_ci(true, sha)?
            }
        }

        self.prep_offline_registry()?;

        Ok(())
    }

    pub fn rustup_name(&self) -> String {
        match *self {
            Toolchain::Dist(ref n) => n.to_string(),
            Toolchain::TryBuild { ref sha } | Toolchain::Master { ref sha } => {
                format!("{}-alt", sha)
            }
        }
    }

    pub fn target_dir(&self, ex_name: &str) -> PathBuf {
        let mut dir = ex_target_dir(ex_name);

        if let Some(thread) = ::std::thread::current().name() {
            dir = dir.join(thread);
        } else {
            dir = dir.join("shared");
        }

        dir.join(self.to_string())
    }

    pub fn run_cargo(
        &self,
        config: &Config,
        ex: &Experiment,
        source_dir: &Path,
        args: &[&str],
        cargo_state: CargoState,
        quiet: bool,
        unstable_cargo: bool,
    ) -> Result<()> {
        let toolchain_name = self.rustup_name();
        let ex_target_dir = self.target_dir(&ex.name);

        fs::create_dir_all(&ex_target_dir)?;

        let toolchain_arg = "+".to_string() + &toolchain_name;
        let mut full_args = vec!["cargo", &*toolchain_arg];
        full_args.extend_from_slice(args);

        info!("running: {}", full_args.join(" "));

        let enable_unstable_cargo_features = !toolchain_name.starts_with("nightly-")
            && (unstable_cargo || args.iter().any(|a| a.starts_with("-Z")));

        let perm = match cargo_state {
            CargoState::Locked => MountPerms::ReadOnly,
            CargoState::Unlocked => MountPerms::ReadWrite,
        };

        let mut container = ContainerBuilder::new(IMAGE_NAME)
            // Setup all the mount points
            .mount(source_dir.into(), "/source", perm)
            .mount(ex_target_dir, "/target", MountPerms::ReadWrite)
            .mount(Path::new(&*CARGO_HOME).into(), "/cargo-home", perm)
            .mount(Path::new(&*RUSTUP_HOME).into(), "/rustup-home", MountPerms::ReadOnly)
            // Add environment variables
            .env("USER_ID", user_id().to_string())
            .env("CMD", full_args.join(" "))
            .env("CARGO_INCREMENTAL", "0".to_string())
            .env("RUST_BACKTRACE", "full".to_string())
            .env("RUSTFLAGS", format!("--cap-lints={}", ex.cap_lints.to_str()))
            // Add some limits to the container
            .memory_limit(&config.sandbox.memory_limit);

        if enable_unstable_cargo_features {
            container = container.env(
                "__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS",
                "nightly".to_string(),
            );
        }

        container.run(quiet)
    }

    pub fn prep_offline_registry(&self) -> Result<()> {
        // This nop cargo command is to update the registry
        // so we don't have to do it for each crate.
        let toolchain_arg = "+".to_string() + &self.rustup_name();
        let full_args = [&toolchain_arg, "search", "lazy_static"];
        RunCommand::new(&installed_binary("cargo"), &full_args)
            .local_rustup()
            .quiet(true)
            .run()
            .chain_err(|| {
                format!(
                    "unable to update the index for toolchain {}",
                    &self.rustup_name()
                )
            })
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
            Ok(Toolchain::TryBuild {
                sha: get_sha(s)?.into(),
            })
        } else if s.starts_with("master#") {
            Ok(Toolchain::Master {
                sha: get_sha(s)?.into(),
            })
        } else {
            Ok(Toolchain::Dist(s.to_string()))
        }
    }
}

fn init_rustup() -> Result<()> {
    fs::create_dir_all(&*CARGO_HOME)?;
    fs::create_dir_all(&*RUSTUP_HOME)?;
    if Path::new(&installed_binary("rustup")).exists() {
        update_rustup()?;
    } else {
        install_rustup()?;
    }

    Ok(())
}

fn installed_binary(name: &str) -> String {
    format!("{}/bin/{}{}", *CARGO_HOME, name, EXE_SUFFIX)
}

fn install_rustup() -> Result<()> {
    info!("installing rustup");
    let rustup_url = &format!(
        "{}/{}/rustup-init{}",
        RUSTUP_BASE_URL,
        &util::this_target(),
        EXE_SUFFIX
    );
    let mut response = dl::download(rustup_url).chain_err(|| "unable to download rustup")?;

    let tempdir = TempDir::new("crater")?;
    let installer = &tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
    {
        let mut file = File::create(installer)?;
        io::copy(&mut response, &mut file)?;
        make_executable(installer)?;
    }

    util::try_hard(|| {
        RunCommand::new(&installer.to_string_lossy(), &["-y", "--no-modify-path"])
            .local_rustup()
            .run()
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
        RunCommand::new(&installed_binary("rustup"), &["self", "update"])
            .local_rustup()
            .run()
            .chain_err(|| "unable to run rustup self-update")
    })
}

fn init_toolchain_from_dist(toolchain: &str) -> Result<()> {
    info!("installing toolchain {}", toolchain);
    util::try_hard(|| {
        RunCommand::new(
            &installed_binary("rustup"),
            &["toolchain", "install", toolchain],
        ).local_rustup()
            .run()
            .chain_err(|| format!("unable to install toolchain {} via rustup", toolchain))
    })
}

fn init_toolchain_from_ci(alt: bool, sha: &str) -> Result<()> {
    // Ensure rustup-toolchain-install-master is installed
    let bin = installed_binary("rustup-toolchain-install-master");
    if !Path::new(&bin).exists() {
        info!("installing rustup-toolchain-install-master");
        util::try_hard(|| {
            RunCommand::new(
                &installed_binary("cargo"),
                &["install", "rustup-toolchain-install-master"],
            ).local_rustup()
                .run()
                .chain_err(|| "unable to install rustup-toolchain-install-master")
        })?;
    }

    if alt {
        info!("installing toolchain {}-alt", sha);
    } else {
        info!("installing toolchain {}", sha);
    }

    let mut args = vec![sha, "-c", "cargo"];
    if alt {
        args.push("--alt");
    }

    util::try_hard(|| {
        RunCommand::new(&bin, &args)
            .local_rustup()
            .run()
            .chain_err(|| {
                format!(
                    "unable to install toolchain {} via rustup-toolchain-install-master",
                    sha
                )
            })
    })
}

#[cfg(unix)]
fn user_id() -> ::libc::uid_t {
    unsafe { ::libc::geteuid() }
}

#[cfg(unix)]
fn user_id() -> u32 {
    unimplemented!();
}
