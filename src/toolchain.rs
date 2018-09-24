use config::Config;
use dirs::{CARGO_HOME, RUSTUP_HOME, TARGET_DIR};
use docker::{ContainerBuilder, MountPerms, IMAGE_NAME};
use errors::*;
use experiments::Experiment;
use run::RunCommand;
use std::env::consts::EXE_SUFFIX;
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tempdir::TempDir;
use utils;

const RUSTUP_BASE_URL: &str = "https://static.rust-lang.org/rustup/dist";

pub fn ex_target_dir(ex_name: &str) -> PathBuf {
    TARGET_DIR.join(ex_name)
}

#[derive(Copy, Clone)]
pub enum CargoState {
    Locked,
    Unlocked,
}

lazy_static! {
    /// This is the main toolchain used by Crater for everything not experiment-specific, such as
    /// generating lockfiles or fetching dependencies.
    pub static ref MAIN_TOOLCHAIN: Toolchain = Toolchain {
        source: ToolchainSource::Dist {
            name: "stable".to_string()
        },
        rustflags: None,
    };
}

#[cfg(test)]
lazy_static! {
    /// This toolchain is used during internal tests, and must be different than MAIN_TOOLCHAIN
    pub static ref TEST_TOOLCHAIN: Toolchain = Toolchain {
        source: ToolchainSource::Dist {
            name: "beta".to_string()
        },
        rustflags: None,
    };
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ToolchainSource {
    Dist {
        name: String,
    },
    #[serde(rename = "ci")]
    CI {
        sha: String,
        try: bool,
    },
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
pub struct Toolchain {
    pub source: ToolchainSource,
    pub rustflags: Option<String>,
}

impl Toolchain {
    pub fn prepare(&self) -> Result<()> {
        init_rustup()?;

        match self.source {
            ToolchainSource::Dist { ref name } => init_toolchain_from_dist(name)?,
            ToolchainSource::CI { ref sha, .. } => init_toolchain_from_ci(true, sha)?,
        }

        self.prep_offline_registry()?;

        Ok(())
    }

    pub fn rustup_name(&self) -> String {
        match self.source {
            ToolchainSource::Dist { ref name } => name.clone(),
            ToolchainSource::CI { ref sha, .. } => format!("{}-alt", sha),
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

    #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
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
            // Add some limits to the container
            .memory_limit(config.sandbox.memory_limit);

        // Set the RUSTFLAGS environment variable
        let mut rustflags = format!("--cap-lints={}", ex.cap_lints.to_str());
        if let Some(ref tc_rustflags) = self.rustflags {
            rustflags.push(' ');
            rustflags.push_str(tc_rustflags);
        }
        container = container.env("RUSTFLAGS", rustflags);

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
        // using `install` is a temporary solution until
        // https://github.com/rust-lang/cargo/pull/5961
        // is ready
        let full_args = [&toolchain_arg, "install", "lazy_static"];
        let _ = RunCommand::new(&installed_binary("cargo"), &full_args)
            .local_rustup()
            .quiet(true)
            .run()
            .chain_err(|| {
                format!(
                    "unable to update the index for toolchain {}",
                    &self.rustup_name()
                )
            });
        // ignore the error untill
        // https://github.com/rust-lang/cargo/pull/5961
        // is ready
        Ok(())
    }
}

impl fmt::Display for Toolchain {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.source {
            ToolchainSource::Dist { ref name } => write!(f, "{}", name)?,
            ToolchainSource::CI { ref sha, try } => if try {
                write!(f, "try#{}", sha)?;
            } else {
                write!(f, "master#{}", sha)?;
            },
        };

        if let Some(ref flag) = self.rustflags {
            write!(f, "+rustflags={}", flag)?;
        }

        Ok(())
    }
}

impl FromStr for Toolchain {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self> {
        let mut parts = input.split('+');

        let raw_source = parts.next().ok_or(ErrorKind::EmptyToolchainName)?;
        let source = if let Some(hash_idx) = raw_source.find('#') {
            let (source_name, sha_with_hash) = raw_source.split_at(hash_idx);

            let sha = (&sha_with_hash[1..]).to_string();
            if sha.is_empty() {
                return Err(ErrorKind::EmptyToolchainName.into());
            }

            match source_name {
                "try" => ToolchainSource::CI { sha, try: true },
                "master" => ToolchainSource::CI { sha, try: false },
                name => return Err(ErrorKind::InvalidToolchainSourceName(name.to_string()).into()),
            }
        } else if raw_source.is_empty() {
            return Err(ErrorKind::EmptyToolchainName.into());
        } else {
            ToolchainSource::Dist {
                name: raw_source.to_string(),
            }
        };

        let mut rustflags = None;
        for part in parts {
            if let Some(equal_idx) = part.find('=') {
                let (flag, value_with_equal) = part.split_at(equal_idx);
                let value = (&value_with_equal[1..]).to_string();

                if value.is_empty() {
                    return Err(ErrorKind::InvalidToolchainFlag(flag.to_string()).into());
                }

                match flag {
                    "rustflags" => rustflags = Some(value),
                    unknown => {
                        return Err(ErrorKind::InvalidToolchainFlag(unknown.to_string()).into())
                    }
                }
            } else {
                return Err(ErrorKind::InvalidToolchainFlag(part.to_string()).into());
            }
        }

        Ok(Toolchain { source, rustflags })
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
        ::HOST_TARGET,
        EXE_SUFFIX
    );
    let mut response = ::utils::http::get(rustup_url).chain_err(|| "unable to download rustup")?;

    let tempdir = TempDir::new("crater")?;
    let installer = &tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
    {
        let mut file = File::create(installer)?;
        io::copy(&mut response, &mut file)?;
        make_executable(installer)?;
    }

    utils::try_hard(|| {
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
    utils::try_hard(|| {
        RunCommand::new(&installed_binary("rustup"), &["self", "update"])
            .local_rustup()
            .run()
            .chain_err(|| "unable to run rustup self-update")
    })
}

fn init_toolchain_from_dist(toolchain: &str) -> Result<()> {
    info!("installing toolchain {}", toolchain);
    utils::try_hard(|| {
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
        utils::try_hard(|| {
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

    utils::try_hard(|| {
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

#[cfg(windows)]
fn user_id() -> u32 {
    unimplemented!();
}

#[cfg(test)]
mod tests {
    use super::{Toolchain, ToolchainSource};
    use std::str::FromStr;

    #[test]
    fn test_string_repr() {
        macro_rules! test_from_str {
            ($($str:expr => $source:expr,)*) => {
                $(
                    // Test parsing without flags
                    test_from_str!($str => Toolchain {
                        source: $source,
                        rustflags: None,
                    });

                    // Test parsing with flags
                    test_from_str!(concat!($str, "+rustflags=foo bar") => Toolchain {
                        source: $source,
                        rustflags: Some("foo bar".to_string()),
                    });
                )*
            };
            ($str:expr => $rust:expr) => {
                // Test parsing from string to rust
                assert_eq!(Toolchain::from_str($str).unwrap(), $rust);

                // Test dumping from rust to string
                assert_eq!(&$rust.to_string(), $str);

                // Test dumping from rust to string to rust
                assert_eq!(Toolchain::from_str($rust.to_string().as_ref()).unwrap(), $rust);
            };
        }

        // Test valid reprs
        test_from_str! {
            "stable" => ToolchainSource::Dist {
                name: "stable".into(),
            },
            "beta-1970-01-01" => ToolchainSource::Dist {
                name: "beta-1970-01-01".into(),
            },
            "nightly-1970-01-01" => ToolchainSource::Dist {
                name: "nightly-1970-01-01".into(),
            },
            "master#0000000000000000000000000000000000000000" => ToolchainSource::CI {
                sha: "0000000000000000000000000000000000000000".into(),
                try: false,
            },
            "try#0000000000000000000000000000000000000000" => ToolchainSource::CI {
                sha: "0000000000000000000000000000000000000000".into(),
                try: true,
            },
        };

        // Test invalid reprs
        assert!(Toolchain::from_str("").is_err());
        assert!(Toolchain::from_str("master#").is_err());
        assert!(Toolchain::from_str("foo#0000000000000000000000000000000000000000").is_err());
        assert!(Toolchain::from_str("stable+rustflags").is_err());
        assert!(Toolchain::from_str("stable+rustflags=").is_err());
        assert!(Toolchain::from_str("stable+donotusethisflag=ever").is_err())
    }
}
