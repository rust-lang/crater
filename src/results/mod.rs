mod db;
#[cfg(test)]
mod dummy;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;

pub use crate::results::db::{DatabaseDB, ProgressData};
#[cfg(test)]
pub use crate::results::dummy::DummyDB;
use crate::toolchain::Toolchain;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rustwide::logging::LogStorage;
use std::collections::BTreeSet;
use std::{fmt, io::Read, io::Write, str::FromStr};

pub trait ReadResults {
    fn load_log(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<EncodedLog>>;
    fn load_test_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>>;
}

pub trait WriteResults {
    fn get_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>>;
    fn update_crate_version(&self, ex: &Experiment, old: &Crate, new: &Crate) -> Fallible<()>;
    fn record_result<F>(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        existing_logs: &LogStorage,
        encoding_type: EncodingType,
        f: F,
    ) -> Fallible<TestResult>
    where
        F: FnOnce() -> Fallible<TestResult>;
}

pub trait DeleteResults {
    fn delete_all_results(&self, ex: &Experiment) -> Fallible<()>;
    fn delete_result(&self, ex: &Experiment, toolchain: &Toolchain, krate: &Crate) -> Fallible<()>;
}

string_enum!(pub enum EncodingType {
    Plain => "plain",
    Gzip => "gzip",
});

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EncodedLog {
    Plain(Vec<u8>),
    Gzip(Vec<u8>),
}

impl EncodedLog {
    pub fn to_plain(&self) -> Fallible<Vec<u8>> {
        match self {
            EncodedLog::Plain(data) => Ok(data.to_vec()),
            EncodedLog::Gzip(data) => {
                let mut decoded_log = GzDecoder::new(data.as_slice());
                let mut new_log = Vec::new();
                decoded_log.read_to_end(&mut new_log)?;
                Ok(new_log)
            }
        }
    }

    pub fn get_encoding_type(&self) -> EncodingType {
        match self {
            EncodedLog::Plain(_) => EncodingType::Plain,
            EncodedLog::Gzip(_) => EncodingType::Gzip,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            EncodedLog::Plain(data) => data,
            EncodedLog::Gzip(data) => data,
        }
    }

    pub fn from_plain_slice(data: &[u8], desired_encoding: EncodingType) -> Fallible<EncodedLog> {
        match desired_encoding {
            EncodingType::Gzip => {
                let mut encoded_log = GzEncoder::new(Vec::new(), Compression::default());
                encoded_log.write_all(data)?;
                let encoded_log = encoded_log.finish()?;
                Ok(EncodedLog::Gzip(encoded_log))
            }
            EncodingType::Plain => Ok(EncodedLog::Plain(data.to_vec())),
        }
    }
}

macro_rules! test_result_enum {
    (pub enum $name:ident {
        with_reason { $($with_reason_name:ident($reason:ident) => $with_reason_repr:expr,)* }
        without_reason { $($reasonless_name:ident => $reasonless_repr:expr,)* }
    }) => {
        #[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub enum $name {
            $($with_reason_name($reason),)*
            $($reasonless_name,)*
        }

        impl FromStr for $name {
            type Err = ::failure::Error;

            fn from_str(input: &str) -> Fallible<Self> {
                // if there is more than one ':' we assume it's part of a failure reason serialization
                let parts: Vec<&str> = input.splitn(2, ':').collect();

                if parts.len() == 1 {
                    match parts[0] {
                        $($with_reason_repr => Ok($name::$with_reason_name($reason::Unknown)),)*
                        $($reasonless_repr => Ok($name::$reasonless_name),)*
                        other => Err(TestResultParseError::UnknownResult(other.into()).into()),
                    }
                } else {
                    match parts[0] {
                        $($reasonless_repr => Err(TestResultParseError::UnexpectedFailureReason.into()),)*
                        $($with_reason_repr => Ok($name::$with_reason_name(parts[1].parse()?)),)*
                        other => Err(TestResultParseError::UnknownResult(other.into()).into()),
                    }
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                match self {
                    $($name::$with_reason_name(reason) => write!(f, "{}:{}", $with_reason_repr, reason),)*
                    $($name::$reasonless_name => write!(f, "{}", $reasonless_repr),)*
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TestResultParseError {
    #[error("unknown test result: {0}")]
    UnknownResult(String),
    #[error("unexpected failure reason")]
    UnexpectedFailureReason,
}

// simplified and lighter version of cargo-metadata::diagnostic::DiagnosticCode
#[derive(Debug, PartialEq, Serialize, Deserialize, Eq, Clone, Hash, PartialOrd, Ord)]
pub struct DiagnosticCode {
    code: String,
}

impl ::std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "{}", self.code)
    }
}

impl DiagnosticCode {
    pub fn from(s: String) -> DiagnosticCode {
        DiagnosticCode { code: s }
    }
}

impl ::std::str::FromStr for DiagnosticCode {
    type Err = ::failure::Error;

    fn from_str(s: &str) -> ::failure::Fallible<DiagnosticCode> {
        Ok(DiagnosticCode {
            code: s.to_string(),
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub enum FailureReason {
    Unknown,
    OOM,
    NoSpace,
    Timeout,
    ICE,
    NetworkAccess,
    CompilerDiagnosticChange,
    CompilerError(BTreeSet<DiagnosticCode>),
    DependsOn(BTreeSet<Crate>),
}

impl Fail for FailureReason {}

impl ::std::fmt::Display for FailureReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            FailureReason::Unknown => write!(f, "unknown"),
            FailureReason::OOM => write!(f, "oom"),
            FailureReason::NoSpace => write!(f, "no-space"),
            FailureReason::Timeout => write!(f, "timeout"),
            FailureReason::ICE => write!(f, "ice"),
            FailureReason::NetworkAccess => write!(f, "network-access"),
            FailureReason::CompilerError(codes) => write!(
                f,
                "compiler-error({})",
                codes
                    .iter()
                    .map(|diag| diag.code.clone())
                    .collect::<Vec<String>>()
                    .join(", "),
            ),
            FailureReason::DependsOn(deps) => write!(
                f,
                "depends-on({})",
                deps.iter()
                    .map(|dep| dep.id())
                    .collect::<Vec<String>>()
                    .join(", "),
            ),
            FailureReason::CompilerDiagnosticChange => write!(f, "compiler-diagnostic-change"),
        }
    }
}

impl ::std::str::FromStr for FailureReason {
    type Err = ::failure::Error;

    fn from_str(s: &str) -> ::failure::Fallible<FailureReason> {
        if let (Some(idx), true) = (s.find('('), s.ends_with(')')) {
            let prefix = &s[..idx];
            let contents = s[idx + 1..s.len() - 1].split(", ");
            match prefix {
                "compiler-error" => Ok(FailureReason::CompilerError(
                    contents
                        .map(|st| DiagnosticCode {
                            code: st.to_string(),
                        })
                        .collect(),
                )),
                "depends-on" => {
                    let mut krates: BTreeSet<Crate> = BTreeSet::new();
                    for krate in contents {
                        krates.insert(krate.parse()?);
                    }
                    Ok(FailureReason::DependsOn(krates))
                }
                _ => bail!("unexpected prefix: {}", prefix),
            }
        } else {
            match s {
                "network-access" => Ok(FailureReason::NetworkAccess),
                "unknown" => Ok(FailureReason::Unknown),
                "oom" => Ok(FailureReason::OOM),
                "timeout" => Ok(FailureReason::Timeout),
                "ice" => Ok(FailureReason::ICE),
                "no-space" => Ok(FailureReason::NoSpace),
                _ => bail!("unexpected value: {}", s),
            }
        }
    }
}

impl FailureReason {
    pub(crate) fn is_spurious(&self) -> bool {
        match *self {
            FailureReason::OOM
            | FailureReason::Timeout
            | FailureReason::NetworkAccess
            | FailureReason::CompilerDiagnosticChange => true,
            FailureReason::CompilerError(_)
            | FailureReason::NoSpace
            | FailureReason::DependsOn(_)
            | FailureReason::Unknown
            | FailureReason::ICE => false,
        }
    }
}

string_enum!(pub enum BrokenReason {
    Unknown => "unknown",
    CargoToml => "cargo-toml",
    Yanked => "yanked",
    MissingDependencies => "missing-deps",
    MissingGitRepository => "missing-git-repository",
});

test_result_enum!(pub enum TestResult {
    with_reason {
        BrokenCrate(BrokenReason) => "broken",
        BuildFail(FailureReason) => "build-fail",
        TestFail(FailureReason) => "test-fail",
    }
    without_reason {
        TestSkipped => "test-skipped",
        TestPass => "test-pass",
        Skipped => "skipped",
        Error => "error",
    }
});

from_into_string!(TestResult);

#[cfg(test)]
mod tests {
    use crate::crates::*;
    use std::collections::BTreeSet;
    use std::str::FromStr;

    #[test]
    fn test_test_result_parsing() {
        use super::{
            FailureReason::*,
            TestResult::{self, *},
        };

        macro_rules! btreeset {
            ($($x:expr),+ $(,)?) => (
                vec![$($x),+].into_iter().collect::<BTreeSet<_>>()
            );
        }

        macro_rules! test_from_str {
            ($($str:expr => $rust:expr,)*) => {
                $(
                    // Test parsing from string to rust
                    assert_eq!(TestResult::from_str($str).unwrap(), $rust);

                    // Test dumping from rust to string
                    assert_eq!(&$rust.to_string(), $str);

                    // Test dumping from rust to string to rust
                    assert_eq!(TestResult::from_str($rust.to_string().as_ref()).unwrap(), $rust);
                )*
            };
        }

        //"build-fail:depends-on()" => BuildFail(DependsOn(vec!["001"])),
        test_from_str! {
            "build-fail:unknown" => BuildFail(Unknown),
            "build-fail:compiler-error(001, 002)" => BuildFail(CompilerError(btreeset!["001".parse().unwrap(), "002".parse().unwrap()])),
            "build-fail:compiler-error(001)" => BuildFail(CompilerError(btreeset!["001".parse().unwrap()])),
            "build-fail:oom" => BuildFail(OOM),
            "build-fail:ice" => BuildFail(ICE),
            "build-fail:no-space" => BuildFail(NoSpace),
            "test-fail:timeout" => TestFail(Timeout),
            "test-pass" => TestPass,
            "error" => Error,
            "build-fail:depends-on(reg/clint/0.2.1)" => BuildFail(DependsOn(btreeset![Crate::Registry(RegistryCrate{name: "clint".to_string(), version: "0.2.1".to_string()})])),
        }

        // Backward compatibility
        assert_eq!(
            TestResult::from_str("build-fail").unwrap(),
            BuildFail(Unknown)
        );
        assert!(TestResult::from_str("error:oom").is_err());
        assert!(TestResult::from_str("build-fail:pleasedonotaddthis").is_err());
    }
}
