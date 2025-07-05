use crate::prelude::*;
use crate::utils;
use regex::Regex;
use rustwide::Toolchain as RustwideToolchain;
use std::fmt;
use std::str::FromStr;

#[cfg(test)]
lazy_static! {
    /// This toolchain is used during internal tests, and must be different than TEST_TOOLCHAIN
    pub(crate) static ref MAIN_TOOLCHAIN: Toolchain = Toolchain {
        source: RustwideToolchain::dist("stable"),
        target: None,
        rustflags: None,
        rustdocflags: None,
        cargoflags: None,
        ci_try: false,
        patches: Vec::new(),
    };

    /// This toolchain is used during internal tests, and must be different than MAIN_TOOLCHAIN
    pub(crate) static ref TEST_TOOLCHAIN: Toolchain = Toolchain {
        source: RustwideToolchain::dist("beta"),
        target: None,
        rustflags: None,
        rustdocflags: None,
        cargoflags: None,
        ci_try: false,
        patches: Vec::new(),
    };
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
pub struct Toolchain {
    pub source: RustwideToolchain,
    pub target: Option<String>,
    pub rustflags: Option<String>,
    pub rustdocflags: Option<String>,
    pub cargoflags: Option<String>,
    pub ci_try: bool,
    pub patches: Vec<CratePatch>,
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
        if let Some(dist) = self.source.as_dist() {
            write!(f, "{}", dist.name())?;
        } else if let Some(ci) = self.source.as_ci() {
            if self.ci_try {
                write!(f, "try#{}", ci.sha())?;
            } else {
                write!(f, "master#{}", ci.sha())?;
            }
        } else {
            panic!("unsupported rustwide toolchain");
        }

        if let Some(ref target) = self.target {
            write!(f, "+target={target}")?;
        }

        if let Some(ref flag) = self.rustflags {
            write!(f, "+rustflags={flag}")?;
        }

        if let Some(ref flag) = self.rustdocflags {
            write!(f, "+rustdocflags={flag}")?;
        }

        if let Some(ref flag) = self.cargoflags {
            write!(f, "+cargoflags={flag}")?;
        }

        for patch in self.patches.iter() {
            write!(f, "+patch={patch}")?;
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolchainParseError {
    #[error("empty toolchain name")]
    EmptyName,
    #[error("invalid toolchain source name: {0}")]
    InvalidSourceName(String),
    #[error("invalid toolchain flag: {0}")]
    InvalidFlag(String),
    #[error("invalid toolchain SHA: {0} is missing a `try#` or `master#` prefix")]
    PrefixMissing(String),
    #[error("invalid url {0:?}: {1}")]
    InvalidUrl(String, url::ParseError),
}

lazy_static! {
    static ref TOOLCHAIN_SHA_RE: Regex = Regex::new(r"^[a-f0-9]{40}$").unwrap();
}

impl FromStr for Toolchain {
    type Err = ToolchainParseError;

    fn from_str(input: &str) -> Result<Self, ToolchainParseError> {
        let mut parts = input.split('+');

        let raw_source = parts.next().ok_or(ToolchainParseError::EmptyName)?;
        let mut ci_try = false;
        let source = if let Some(hash_idx) = raw_source.find('#') {
            let (source_name, sha_with_hash) = raw_source.split_at(hash_idx);

            let sha = &sha_with_hash[1..];
            if sha.is_empty() {
                return Err(ToolchainParseError::EmptyName);
            }

            match source_name {
                "try" => {
                    ci_try = true;
                    RustwideToolchain::ci(sha, false)
                }
                "master" => RustwideToolchain::ci(sha, false),
                name => return Err(ToolchainParseError::InvalidSourceName(name.to_string())),
            }
        } else if raw_source.is_empty() {
            return Err(ToolchainParseError::EmptyName);
        } else if TOOLCHAIN_SHA_RE.is_match(raw_source) {
            // A common user error is unprefixed SHAs for the `start` or `end` toolchains, check for
            // these here.
            return Err(ToolchainParseError::PrefixMissing(raw_source.to_string()));
        } else {
            RustwideToolchain::dist(raw_source)
        };

        let mut rustflags = None;
        let mut rustdocflags = None;
        let mut cargoflags = None;
        let mut patches: Vec<CratePatch> = vec![];
        let mut target = None;
        for part in parts {
            if let Some(equal_idx) = part.find('=') {
                let (flag, value_with_equal) = part.split_at(equal_idx);
                let value = value_with_equal[1..].to_string();

                if value.is_empty() {
                    return Err(ToolchainParseError::InvalidFlag(flag.to_string()));
                }

                match flag {
                    "rustflags" => rustflags = Some(value),
                    "rustdocflags" => rustdocflags = Some(value),
                    "cargoflags" => cargoflags = Some(value),
                    "patch" => patches.push(value.parse()?),
                    "target" => target = Some(value),
                    unknown => return Err(ToolchainParseError::InvalidFlag(unknown.to_string())),
                }
            } else {
                return Err(ToolchainParseError::InvalidFlag(part.to_string()));
            }
        }

        Ok(Toolchain {
            source,
            target,
            rustflags,
            rustdocflags,
            cargoflags,
            ci_try,
            patches,
        })
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
pub struct CratePatch {
    pub name: String,
    // cargo currently doesn't accept scp-style "URLs" rust-lang/crates#1851
    // so ensure its a proper URL
    pub repo: url::Url,
    pub branch: String,
}

impl FromStr for CratePatch {
    type Err = ToolchainParseError;

    fn from_str(input: &str) -> Result<Self, ToolchainParseError> {
        let params: Vec<&str> = input.split('=').collect();

        if params.len() != 3 {
            Err(ToolchainParseError::InvalidFlag(input.to_string()))
        } else {
            Ok(CratePatch {
                name: params[0].into(),
                repo: url::Url::parse(params[1])
                    .map_err(|err| ToolchainParseError::InvalidUrl(params[1].into(), err))?,
                branch: params[2].into(),
            })
        }
    }
}

impl fmt::Display for CratePatch {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}={}={}", self.name, self.repo, self.branch)
    }
}

#[cfg(test)]
mod tests {
    use super::{CratePatch, Toolchain};
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
                        target: None,
                        rustflags: None,
                        rustdocflags: None,
                        cargoflags: None,
                        ci_try: $ci_try,
                        patches: Vec::new(),
                    });

                    // Test parsing with target
                    test_from_str!(concat!($str, "+target=x86_64-unknown-linux-gnu") => Toolchain {
                        source: $source,
                        target: Some("x86_64-unknown-linux-gnu".to_string()),
                        rustflags: None,
                        rustdocflags: None,
                        cargoflags: None,
                        ci_try: $ci_try,
                        patches: Vec::new(),
                    });

                    // Test parsing with rustflags
                    test_from_str!(concat!($str, "+rustflags=foo bar") => Toolchain {
                        source: $source,
                        target: None,
                        rustflags: Some("foo bar".to_string()),
                        rustdocflags: None,
                        cargoflags: None,
                        ci_try: $ci_try,
                        patches: Vec::new(),
                    });

                    // Test parsing with rustdocflags
                    test_from_str!(concat!($str, "+rustdocflags=-Zunstable-options -wjson") => Toolchain {
                        source: $source,
                        target: None,
                        rustflags: None,
                        rustdocflags: Some("-Zunstable-options -wjson".to_string()),
                        cargoflags: None,
                        ci_try: $ci_try,
                        patches: Vec::new(),
                    });

                    // Test parsing with cargoflags
                    test_from_str!(concat!($str, "+cargoflags=foo bar") => Toolchain {
                        source: $source,
                        target: None,
                        rustflags: None,
                        rustdocflags: None,
                        cargoflags: Some("foo bar".to_string()),
                        ci_try: $ci_try,
                        patches: Vec::new(),
                    });

                    // Test parsing with patches
                    test_from_str!(concat!($str, "+patch=example=https://git.example.com/some/repo=master") => Toolchain {
                        source: $source,
                        target: None,
                        rustflags: None,
                        rustdocflags: None,
                        cargoflags: None,
                        ci_try: $ci_try,
                        patches: vec![CratePatch {
                            name: "example".to_string(),
                            repo: url::Url::parse("https://git.example.com/some/repo").unwrap(),
                            branch: "master".to_string()
                        }]
                    });

                    // Test parsing with patches & rustflags
                    test_from_str!(concat!($str, "+rustflags=foo bar+patch=example=https://git.example.com/some/repo=master") => Toolchain {
                        source: $source,
                        target: None,
                        rustflags: Some("foo bar".to_string()),
                        rustdocflags: None,
                        cargoflags: None,
                        ci_try: $ci_try,
                        patches: vec![CratePatch {
                            name: "example".to_string(),
                            repo: url::Url::parse("https://git.example.com/some/repo").unwrap(),
                            branch: "master".to_string()
                        }]
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
                source: RustwideToolchain::dist("stable"),
                ci_try: false,
            },
            "beta-1970-01-01" => {
                source: RustwideToolchain::dist("beta-1970-01-01"),
                ci_try: false,
            },
            "nightly-1970-01-01" => {
                source: RustwideToolchain::dist("nightly-1970-01-01"),
                ci_try: false,
            },
            "master#0000000000000000000000000000000000000000" => {
                source: RustwideToolchain::ci("0000000000000000000000000000000000000000", false),
                ci_try: false,
            },
            "try#0000000000000000000000000000000000000000" => {
                source: RustwideToolchain::ci("0000000000000000000000000000000000000000", false),
                ci_try: true,
            },
        };

        // Test invalid reprs
        assert!(Toolchain::from_str("").is_err());
        assert!(Toolchain::from_str("master#").is_err());
        assert!(Toolchain::from_str("foo#0000000000000000000000000000000000000000").is_err());
        assert!(Toolchain::from_str("stable+rustflags").is_err());
        assert!(Toolchain::from_str("stable+rustflags=").is_err());
        assert!(Toolchain::from_str("stable+rustdocflags").is_err());
        assert!(Toolchain::from_str("stable+rustdocflags=").is_err());
        assert!(Toolchain::from_str("stable+donotusethisflag=ever").is_err());
        assert!(Toolchain::from_str("stable+patch=").is_err());
        assert!(matches!(
            Toolchain::from_str(
                "stable+patch=getrandom=git@github.com:rust-random/getrandom=backports/v0.2"
            )
            .unwrap_err(),
            super::ToolchainParseError::InvalidUrl(..)
        ));
        assert!(Toolchain::from_str("try#1234+target=").is_err());
        assert!(Toolchain::from_str("0000000000000000000000000000000000000000").is_err());
    }
}
