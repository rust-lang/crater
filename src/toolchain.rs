use crate::prelude::*;
use crate::utils;
use rustwide::Toolchain as RustwideToolchain;
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// This toolchain is used during internal tests, and must be different than TEST_TOOLCHAIN
#[cfg(test)]
pub(crate) static MAIN_TOOLCHAIN: Toolchain = Toolchain {
    source: RustwideToolchain::Dist {
        name: Cow::Borrowed("stable"),
    },
    rustflags: None,
    ci_try: false,
};

/// This toolchain is used during internal tests, and must be different than MAIN_TOOLCHAIN
#[cfg(test)]
pub(crate) static TEST_TOOLCHAIN: Toolchain = Toolchain {
    source: RustwideToolchain::Dist {
        name: Cow::Borrowed("beta"),
    },
    rustflags: None,
    ci_try: false,
};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
pub struct Toolchain {
    pub source: RustwideToolchain,
    pub rustflags: Option<String>,
    pub ci_try: bool,
}

impl Toolchain {
    pub fn to_path_component(&self) -> String {
        use percent_encoding::utf8_percent_encode as encode;

        encode(&self.to_string(), &utils::FILENAME_ENCODE_SET).to_string()
    }
}

impl std::ops::Deref for Toolchain {
    type Target = RustwideToolchain;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

impl fmt::Display for Toolchain {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.source {
            RustwideToolchain::Dist { name } => write!(f, "{}", name)?,
            RustwideToolchain::CI { sha, .. } => {
                if self.ci_try {
                    write!(f, "try#{}", sha)?;
                } else {
                    write!(f, "master#{}", sha)?;
                }
            }
            _ => panic!("unsupported rustwide toolchain"),
        };

        if let Some(ref flag) = self.rustflags {
            write!(f, "+rustflags={}", flag)?;
        }

        Ok(())
    }
}

#[derive(Debug, Fail)]
pub enum ToolchainParseError {
    #[fail(display = "empty toolchain name")]
    EmptyName,
    #[fail(display = "invalid toolchain source name: {}", _0)]
    InvalidSourceName(String),
    #[fail(display = "invalid toolchain flag: {}", _0)]
    InvalidFlag(String),
}

impl FromStr for Toolchain {
    type Err = ToolchainParseError;

    fn from_str(input: &str) -> Result<Self, ToolchainParseError> {
        let mut parts = input.split('+');

        let raw_source = parts.next().ok_or(ToolchainParseError::EmptyName)?;
        let mut ci_try = false;
        let source = if let Some(hash_idx) = raw_source.find('#') {
            let (source_name, sha_with_hash) = raw_source.split_at(hash_idx);

            let sha = (&sha_with_hash[1..]).to_string();
            if sha.is_empty() {
                return Err(ToolchainParseError::EmptyName);
            }

            match source_name {
                "try" => {
                    ci_try = true;
                    RustwideToolchain::CI {
                        sha: Cow::Owned(sha),
                        alt: true,
                    }
                }
                "master" => RustwideToolchain::CI {
                    sha: Cow::Owned(sha),
                    alt: true,
                },
                name => return Err(ToolchainParseError::InvalidSourceName(name.to_string())),
            }
        } else if raw_source.is_empty() {
            return Err(ToolchainParseError::EmptyName);
        } else {
            RustwideToolchain::Dist {
                name: Cow::Owned(raw_source.to_string()),
            }
        };

        let mut rustflags = None;
        for part in parts {
            if let Some(equal_idx) = part.find('=') {
                let (flag, value_with_equal) = part.split_at(equal_idx);
                let value = (&value_with_equal[1..]).to_string();

                if value.is_empty() {
                    return Err(ToolchainParseError::InvalidFlag(flag.to_string()));
                }

                match flag {
                    "rustflags" => rustflags = Some(value),
                    unknown => return Err(ToolchainParseError::InvalidFlag(unknown.to_string())),
                }
            } else {
                return Err(ToolchainParseError::InvalidFlag(part.to_string()));
            }
        }

        Ok(Toolchain {
            source,
            rustflags,
            ci_try,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Toolchain;
    use rustwide::Toolchain as RustwideToolchain;
    use std::str::FromStr;

    #[test]
    fn test_string_repr() {
        macro_rules! test_from_str {
            ($($str:expr => { source: $source:expr, ci_try: $ci_try:expr, },)*) => {
                $(
                    // Test parsing without flags
                    test_from_str!($str => Toolchain {
                        source: $source,
                        rustflags: None,
                        ci_try: $ci_try,
                    });

                    // Test parsing with flags
                    test_from_str!(concat!($str, "+rustflags=foo bar") => Toolchain {
                        source: $source,
                        rustflags: Some("foo bar".to_string()),
                        ci_try: $ci_try,
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
            "stable" => {
                source:RustwideToolchain::Dist {
                    name: "stable".into(),
                },
                ci_try: false,
            },
            "beta-1970-01-01" => {
                source: RustwideToolchain::Dist {
                    name: "beta-1970-01-01".into(),
                },
                ci_try: false,
            },
            "nightly-1970-01-01" => {
                source: RustwideToolchain::Dist {
                    name: "nightly-1970-01-01".into(),
                },
                ci_try: false,
            },
            "master#0000000000000000000000000000000000000000" => {
                source: RustwideToolchain::CI {
                    sha: "0000000000000000000000000000000000000000".into(),
                    alt: true,
                },
                ci_try: false,
            },
            "try#0000000000000000000000000000000000000000" => {
                source: RustwideToolchain::CI {
                    sha: "0000000000000000000000000000000000000000".into(),
                    alt: true,
                },
                ci_try: true,
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
