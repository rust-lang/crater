use dirs::TARGET_DIR;
use errors::*;
use run::RunCommand;
use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use tools::CARGO;
use tools::{RUSTUP, RUSTUP_TOOLCHAIN_INSTALL_MASTER};
use utils;

pub(crate) static MAIN_TOOLCHAIN_NAME: &str = "stable";

pub fn ex_target_dir(ex_name: &str) -> PathBuf {
    TARGET_DIR.join(ex_name)
}

/// This is the main toolchain used by Crater for everything not experiment-specific, such as
/// generating lockfiles or fetching dependencies.
pub(crate) static MAIN_TOOLCHAIN: Toolchain = Toolchain {
    source: ToolchainSource::Dist {
        name: Cow::Borrowed(MAIN_TOOLCHAIN_NAME),
    },
    rustflags: None,
};

/// This toolchain is used during internal tests, and must be different than MAIN_TOOLCHAIN
#[cfg(test)]
pub(crate) static TEST_TOOLCHAIN: Toolchain = Toolchain {
    source: ToolchainSource::Dist {
        name: Cow::Borrowed("beta"),
    },
    rustflags: None,
};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ToolchainSource {
    Dist {
        name: Cow<'static, str>,
    },
    #[serde(rename = "ci")]
    CI {
        sha: Cow<'static, str>,
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
        match self.source {
            ToolchainSource::Dist { ref name } => init_toolchain_from_dist(name)?,
            ToolchainSource::CI { ref sha, .. } => init_toolchain_from_ci(true, sha)?,
        }

        self.prep_offline_registry()?;

        Ok(())
    }

    pub fn rustup_name(&self) -> String {
        match self.source {
            ToolchainSource::Dist { ref name } => name.to_string(),
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

    pub fn prep_offline_registry(&self) -> Result<()> {
        // This nop cargo command is to update the registry
        // so we don't have to do it for each crate.
        // using `install` is a temporary solution until
        // https://github.com/rust-lang/cargo/pull/5961
        // is ready

        let _ = RunCommand::new(CARGO.toolchain(self))
            .args(&["install", "lazy_static"])
            .quiet(true)
            .run();

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
                "try" => ToolchainSource::CI {
                    sha: Cow::Owned(sha),
                    try: true,
                },
                "master" => ToolchainSource::CI {
                    sha: Cow::Owned(sha),
                    try: false,
                },
                name => return Err(ErrorKind::InvalidToolchainSourceName(name.to_string()).into()),
            }
        } else if raw_source.is_empty() {
            return Err(ErrorKind::EmptyToolchainName.into());
        } else {
            ToolchainSource::Dist {
                name: Cow::Owned(raw_source.to_string()),
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

fn init_toolchain_from_dist(toolchain: &str) -> Result<()> {
    info!("installing toolchain {}", toolchain);
    utils::try_hard(|| {
        RunCommand::new(&RUSTUP)
            .args(&["toolchain", "install", toolchain])
            .run()
            .chain_err(|| format!("unable to install toolchain {} via rustup", toolchain))
    })
}

fn init_toolchain_from_ci(alt: bool, sha: &str) -> Result<()> {
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
        RunCommand::new(&RUSTUP_TOOLCHAIN_INSTALL_MASTER)
            .args(&args)
            .run()
            .chain_err(|| {
                format!(
                    "unable to install toolchain {} via rustup-toolchain-install-master",
                    sha
                )
            })
    })
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
