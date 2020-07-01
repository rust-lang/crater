use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::report::analyzer::{analyze_report, ReportConfig, ToolchainSelect};
use crate::results::{EncodedLog, EncodingType, FailureReason, ReadResults, TestResult};
use crate::toolchain::Toolchain;
use crate::utils;
use mime::{self, Mime};
use percent_encoding::{utf8_percent_encode, AsciiSet};
use std::borrow::Cow;
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
use std::convert::AsRef;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

mod analyzer;
mod archives;
mod display;
mod html;
mod markdown;
mod s3;

pub use self::display::{Color, ResultColor, ResultName};
pub use self::s3::{get_client_for_bucket, S3Prefix, S3Writer};
pub use analyzer::TestResults;

pub(crate) const REPORT_ENCODE_SET: AsciiSet = percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'?')
    .add(b'{')
    .add(b'}')
    .add(b'+');

#[derive(Serialize, Deserialize)]
pub struct RawTestResults {
    pub crates: Vec<CrateResult>,
}

#[cfg_attr(test, derive(Debug))]
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct CrateResult {
    name: String,
    url: String,
    krate: Crate,
    pub res: Comparison,
    runs: [Option<BuildTestResult>; 2],
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportPriority {
    Low,
    Medium,
    High,
}

string_enum!(pub enum Comparison {
    Regressed => "regressed",
    Fixed => "fixed",
    Skipped => "skipped",
    Unknown => "unknown",
    Error => "error",
    Broken => "broken",
    SameBuildFail => "build-fail",
    SameTestFail => "test-fail",
    SameTestSkipped => "test-skipped",
    SameTestPass => "test-pass",
    SpuriousRegressed => "spurious-regressed",
    SpuriousFixed => "spurious-fixed",
});

impl Comparison {
    pub fn report_priority(self) -> ReportPriority {
        match self {
            Comparison::Regressed | Comparison::Fixed => ReportPriority::High,
            Comparison::Unknown
            | Comparison::Error
            | Comparison::SpuriousRegressed
            | Comparison::SpuriousFixed => ReportPriority::Medium,
            Comparison::Skipped
            | Comparison::Broken
            | Comparison::SameBuildFail
            | Comparison::SameTestFail
            | Comparison::SameTestSkipped
            | Comparison::SameTestPass => ReportPriority::Low,
        }
    }

    pub fn report_config(self) -> ReportConfig {
        match self {
            Comparison::Regressed => ReportConfig::Complete(ToolchainSelect::End),
            Comparison::Fixed => ReportConfig::Complete(ToolchainSelect::Start),
            Comparison::Unknown
            | Comparison::Error
            | Comparison::SpuriousRegressed
            | Comparison::SpuriousFixed
            | Comparison::Skipped
            | Comparison::Broken
            | Comparison::SameBuildFail
            | Comparison::SameTestFail
            | Comparison::SameTestSkipped
            | Comparison::SameTestPass => ReportConfig::Simple,
        }
    }
}

#[cfg_attr(test, derive(Debug))]
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
struct BuildTestResult {
    res: TestResult,
    log: String,
}

/// The type of sanitization required for a string.
#[derive(Debug, Clone, Copy)]
enum SanitizationContext {
    Url,
    Path,
}

impl SanitizationContext {
    fn sanitize(self, input: &str) -> Cow<str> {
        match self {
            SanitizationContext::Url => utf8_percent_encode(input, &REPORT_ENCODE_SET).into(),

            SanitizationContext::Path => {
                utf8_percent_encode(input, &utils::FILENAME_ENCODE_SET).into()
            }
        }
    }
}

fn crate_to_path_fragment(
    toolchain: &Toolchain,
    krate: &Crate,
    dest: SanitizationContext,
) -> PathBuf {
    let mut path = PathBuf::new();
    path.push(dest.sanitize(&toolchain.to_string()).into_owned());

    match *krate {
        Crate::Registry(ref details) => {
            path.push("reg");

            let name = format!("{}-{}", details.name, details.version);
            path.push(dest.sanitize(&name).into_owned());
        }
        Crate::GitHub(ref repo) => {
            path.push("gh");

            let name = format!("{}.{}", repo.org, repo.name);
            path.push(dest.sanitize(&name).into_owned());
        }
        Crate::Local(ref name) => {
            path.push("local");
            path.push(name);
        }
        Crate::Path(ref krate_path) => {
            path.push("path");
            path.push(dest.sanitize(&krate_path).into_owned());
        }
        Crate::Git(ref repo) => {
            path.push("git");
            path.push(dest.sanitize(&repo.url).into_owned());
        }
    }

    path
}

pub fn generate_report<DB: ReadResults>(
    db: &DB,
    config: &Config,
    ex: &Experiment,
    crates: &[Crate],
) -> Fallible<RawTestResults> {
    let mut crates = crates.to_vec();
    //crate ids are unique so unstable sort is equivalent to stable sort but is generally faster
    crates.sort_unstable_by(|a, b| a.id().cmp(&b.id()));
    let res = crates
        .iter()
        .map(|krate| {
            // Any errors here will turn into unknown results
            let crate_results = ex.toolchains.iter().map(|tc| -> Fallible<BuildTestResult> {
                let res = db
                    .load_test_result(ex, tc, &krate)?
                    .ok_or_else(|| err_msg("no result"))?;

                Ok(BuildTestResult {
                    res,
                    log: crate_to_path_fragment(tc, &krate, SanitizationContext::Url)
                        .to_str()
                        .unwrap()
                        .replace(r"\", "/"), // Normalize paths in reports generated on Windows
                })
            });
            // Convert errors to Nones
            let mut crate_results = crate_results.map(|r| r.ok()).collect::<Vec<_>>();
            let crate2 = crate_results.pop().unwrap();
            let crate1 = crate_results.pop().unwrap();
            let comp = compare(
                config,
                &krate,
                crate1.as_ref().map(|b| &b.res),
                crate2.as_ref().map(|b| &b.res),
            );

            Ok(CrateResult {
                name: crate_to_name(&krate)?,
                url: crate_to_url(&krate)?,
                krate: krate.clone(),
                res: comp,
                runs: [crate1, crate2],
            })
        })
        .collect::<Fallible<Vec<_>>>()?;

    Ok(RawTestResults { crates: res })
}

const PROGRESS_FRACTION: usize = 10; // write progress every ~1/N crates

fn write_logs<DB: ReadResults, W: ReportWriter>(
    db: &DB,
    ex: &Experiment,
    crates: &[Crate],
    dest: &W,
    config: &Config,
) -> Fallible<()> {
    let num_crates = crates.len();
    let progress_every = (num_crates / PROGRESS_FRACTION) + 1;
    for (i, krate) in crates.iter().enumerate() {
        if i % progress_every == 0 {
            info!("wrote logs for {}/{} crates", i, num_crates)
        }

        if config.should_skip(krate) {
            continue;
        }

        for tc in &ex.toolchains {
            let log_path =
                crate_to_path_fragment(tc, krate, SanitizationContext::Path).join("log.txt");
            let content = db
                .load_log(ex, tc, krate)
                .and_then(|c| c.ok_or_else(|| err_msg("missing logs")))
                .with_context(|_| format!("failed to read log of {} on {}", krate, tc.to_string()));
            let content = match content {
                Ok(c) => c,
                Err(e) => {
                    utils::report_failure(&e);
                    continue;
                }
            };

            match content {
                EncodedLog::Plain(data) => {
                    dest.write_bytes(log_path, data, &mime::TEXT_PLAIN_UTF_8, EncodingType::Plain)
                }
                EncodedLog::Gzip(data) => {
                    dest.write_bytes(log_path, data, &mime::TEXT_PLAIN_UTF_8, EncodingType::Gzip)
                }
            }?;
        }
    }
    Ok(())
}

pub fn gen<DB: ReadResults, W: ReportWriter + Display>(
    db: &DB,
    ex: &Experiment,
    crates: &[Crate],
    dest: &W,
    config: &Config,
    output_templates: bool,
) -> Fallible<TestResults> {
    let raw = generate_report(db, config, ex, crates)?;

    info!("writing results to {}", dest);
    info!("writing metadata");
    dest.write_string(
        "results.json",
        serde_json::to_string(&raw)?.into(),
        &mime::APPLICATION_JSON,
    )?;
    dest.write_string(
        "config.json",
        serde_json::to_string(&ex)?.into(),
        &mime::APPLICATION_JSON,
    )?;
    dest.write_string(
        "retry-regressed-list.txt",
        gen_retry_list(&raw).into(),
        &mime::TEXT_PLAIN_UTF_8,
    )?;

    let res = analyze_report(raw);
    info!("writing archives");
    let available_archives = archives::write_logs_archives(db, ex, crates, dest, config)?;
    info!("writing html files");
    html::write_html_report(
        ex,
        crates.len(),
        &res,
        available_archives,
        dest,
        output_templates,
    )?;
    info!("writing markdown files");
    markdown::write_markdown_report(ex, crates.len(), &res, dest, output_templates)?;
    info!("writing logs");
    write_logs(db, ex, crates, dest, config)?;

    Ok(res)
}

/// Generates a list of regressed crate names that can be passed to crater via
/// `crates=list:...` to retry those.
fn gen_retry_list(res: &RawTestResults) -> String {
    use std::fmt::Write;

    let mut out = String::new();

    let regressed_crates = res
        .crates
        .iter()
        .filter(|crate_res| crate_res.res == Comparison::Regressed)
        .map(|crate_res| &crate_res.krate);

    for krate in regressed_crates {
        match krate {
            Crate::Registry(details) => writeln!(out, "{}", details.name).unwrap(),
            Crate::GitHub(repo) => writeln!(out, "{}/{}", repo.org, repo.name).unwrap(),
            Crate::Local(_) | Crate::Git(_) | Crate::Path(_) => {}
        }
    }

    out
}

fn crate_to_name(c: &Crate) -> Fallible<String> {
    Ok(match *c {
        Crate::Registry(ref details) => format!("{}-{}", details.name, details.version),
        Crate::GitHub(ref repo) => {
            if let Some(ref sha) = repo.sha {
                format!("{}.{}.{}", repo.org, repo.name, sha)
            } else {
                format!("{}.{}", repo.org, repo.name)
            }
        }
        Crate::Local(ref name) => format!("{} (local)", name),
        Crate::Path(ref path) => utf8_percent_encode(path, &REPORT_ENCODE_SET).to_string(),
        Crate::Git(ref repo) => {
            if let Some(ref sha) = repo.sha {
                format!(
                    "{}.{}",
                    utf8_percent_encode(&repo.url, &REPORT_ENCODE_SET),
                    sha
                )
            } else {
                utf8_percent_encode(&repo.url, &REPORT_ENCODE_SET).to_string()
            }
        }
    })
}

fn crate_to_url(c: &Crate) -> Fallible<String> {
    Ok(match *c {
        Crate::Registry(ref details) => format!(
            "https://crates.io/crates/{}/{}",
            details.name, details.version
        ),
        Crate::GitHub(ref repo) => {
            if let Some(ref sha) = repo.sha {
                format!("https://github.com/{}/{}/tree/{}", repo.org, repo.name, sha)
            } else {
                format!("https://github.com/{}/{}", repo.org, repo.name)
            }
        }
        Crate::Local(ref name) => format!(
            "{}/tree/master/local-crates/{}",
            crate::CRATER_REPO_URL,
            name
        ),
        Crate::Path(ref path) => utf8_percent_encode(path, &REPORT_ENCODE_SET).to_string(),
        Crate::Git(ref repo) => repo.url.clone(),
    })
}

fn compare(
    config: &Config,
    krate: &Crate,
    r1: Option<&TestResult>,
    r2: Option<&TestResult>,
) -> Comparison {
    use crate::results::TestResult::*;

    match (r1, r2) {
        (Some(res1), Some(res2)) => match (res1, res2) {
            // ICE -> ICE is not a regression, but anything else to an ICE is.
            (BuildFail(FailureReason::ICE), BuildFail(FailureReason::ICE)) => {
                Comparison::SameBuildFail
            }
            (BuildFail(_), BuildFail(FailureReason::ICE)) => Comparison::Regressed,

            (BuildFail(_), BuildFail(_)) => Comparison::SameBuildFail,
            (TestFail(_), TestFail(_)) => Comparison::SameTestFail,
            (TestSkipped, TestSkipped) => Comparison::SameTestSkipped,
            (TestPass, TestPass) => Comparison::SameTestPass,

            (BuildFail(ref reason1), TestFail(ref reason2))
                if reason1.is_spurious() || reason2.is_spurious() =>
            {
                Comparison::SpuriousFixed
            }
            (BuildFail(ref reason), TestSkipped)
            | (BuildFail(ref reason), TestPass)
            | (TestFail(ref reason), TestPass)
                if reason.is_spurious() =>
            {
                Comparison::SpuriousFixed
            }
            (BuildFail(_), TestFail(_))
            | (BuildFail(_), TestSkipped)
            | (BuildFail(_), TestPass)
            | (TestFail(_), TestPass) => Comparison::Fixed,

            (TestFail(reason1), BuildFail(reason2))
                if reason1.is_spurious() || reason2.is_spurious() =>
            {
                Comparison::SpuriousRegressed
            }
            (TestPass, TestFail(reason))
            | (TestPass, BuildFail(reason))
            | (TestSkipped, BuildFail(reason))
            | (TestFail(_), BuildFail(reason))
                if reason.is_spurious() =>
            {
                Comparison::SpuriousRegressed
            }
            (TestPass, TestFail(_))
            | (TestPass, BuildFail(_))
            | (TestSkipped, BuildFail(_))
            | (TestFail(_), BuildFail(_)) => Comparison::Regressed,

            (Error, _) | (_, Error) => Comparison::Error,
            (Skipped, _) | (_, Skipped) => Comparison::Skipped,
            (BrokenCrate(_), _) | (_, BrokenCrate(_)) => Comparison::Broken,
            (TestFail(_), TestSkipped)
            | (TestPass, TestSkipped)
            | (TestSkipped, TestFail(_))
            | (TestSkipped, TestPass) => {
                panic!("can't compare {} and {}", res1, res2);
            }
        },
        _ if config.should_skip(krate) => Comparison::Skipped,
        _ => Comparison::Unknown,
    }
}

pub trait ReportWriter {
    fn write_bytes<P: AsRef<Path>>(
        &self,
        path: P,
        b: Vec<u8>,
        mime: &Mime,
        encoding_type: EncodingType,
    ) -> Fallible<()>;
    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, mime: &Mime) -> Fallible<()>;
    fn copy<P: AsRef<Path>, R: Read>(&self, r: &mut R, path: P, mime: &Mime) -> Fallible<()>;
}

pub struct FileWriter(PathBuf);

impl FileWriter {
    pub fn create(dest: PathBuf) -> Fallible<FileWriter> {
        fs::create_dir_all(&dest)?;
        Ok(FileWriter(dest))
    }
    fn create_prefix(&self, path: &Path) -> Fallible<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(self.0.join(parent))?;
        }
        Ok(())
    }
}

impl ReportWriter for FileWriter {
    fn write_bytes<P: AsRef<Path>>(
        &self,
        path: P,
        b: Vec<u8>,
        _: &Mime,
        _: EncodingType,
    ) -> Fallible<()> {
        self.create_prefix(path.as_ref())?;
        fs::write(&self.0.join(path.as_ref()), &b)?;
        Ok(())
    }

    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, _: &Mime) -> Fallible<()> {
        self.create_prefix(path.as_ref())?;
        fs::write(&self.0.join(path.as_ref()), s.as_ref().as_bytes())?;
        Ok(())
    }

    fn copy<P: AsRef<Path>, R: Read>(&self, r: &mut R, path: P, _: &Mime) -> Fallible<()> {
        self.create_prefix(path.as_ref())?;
        io::copy(r, &mut File::create(self.0.join(path.as_ref()))?)?;
        Ok(())
    }
}

impl Display for FileWriter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.display().fmt(f)
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct DummyWriter {
    results: RefCell<HashMap<(PathBuf, Mime), Vec<u8>>>,
}

#[cfg(test)]
impl DummyWriter {
    pub fn get<P: AsRef<Path>>(&self, path: P, mime: &Mime) -> Vec<u8> {
        self.results
            .borrow()
            .get(&(path.as_ref().to_path_buf(), mime.clone()))
            .unwrap()
            .clone()
    }
}

#[cfg(test)]
impl ReportWriter for DummyWriter {
    fn write_bytes<P: AsRef<Path>>(
        &self,
        path: P,
        b: Vec<u8>,
        mime: &Mime,
        _: EncodingType,
    ) -> Fallible<()> {
        self.results
            .borrow_mut()
            .insert((path.as_ref().to_path_buf(), mime.clone()), b);
        Ok(())
    }

    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, mime: &Mime) -> Fallible<()> {
        self.results.borrow_mut().insert(
            (path.as_ref().to_path_buf(), mime.clone()),
            s.bytes().collect(),
        );
        Ok(())
    }

    fn copy<P: AsRef<Path>, R: Read>(&self, r: &mut R, path: P, mime: &Mime) -> Fallible<()> {
        let mut buffer = Vec::new();
        r.read_to_end(&mut buffer)?;

        self.results
            .borrow_mut()
            .insert((path.as_ref().to_path_buf(), mime.clone()), buffer);
        Ok(())
    }
}

#[cfg(test)]
impl Display for DummyWriter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, ":dummy:")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, CrateConfig};
    use crate::crates::{Crate, GitHubRepo, RegistryCrate};
    use crate::experiments::{CapLints, Experiment, Mode, Status};
    use crate::results::{BrokenReason, DummyDB, FailureReason, TestResult};
    use crate::toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    #[test]
    fn test_crate_to_path_fragment() {
        let reg = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.0".into(),
        });
        let gh = Crate::GitHub(GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
            sha: None,
        });
        let gt_plus = Crate::Registry(RegistryCrate {
            name: "foo".into(),
            version: ">1.0+bar".into(),
        });

        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &reg, SanitizationContext::Path),
            PathBuf::from("stable/reg/lazy_static-1.0")
        );
        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &gh, SanitizationContext::Path),
            PathBuf::from("stable/gh/brson.hello-rs")
        );
        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &gt_plus, SanitizationContext::Path),
            PathBuf::from("stable/reg/foo-%3E1.0+bar")
        );
        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &gt_plus, SanitizationContext::Url),
            PathBuf::from("stable/reg/foo-%3E1.0%2Bbar")
        );
    }

    #[test]
    fn test_crate_to_name() {
        let reg = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.0".into(),
        });
        assert_eq!(crate_to_name(&reg).unwrap(), "lazy_static-1.0".to_string());

        let repo = GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
            sha: None,
        };
        let gh = Crate::GitHub(repo);

        assert_eq!(crate_to_name(&gh).unwrap(), "brson.hello-rs".to_string());

        let repo = GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
            sha: Some("f00".into()),
        };
        let gh = Crate::GitHub(repo);

        assert_eq!(
            crate_to_name(&gh).unwrap(),
            "brson.hello-rs.f00".to_string()
        );
    }

    #[test]
    fn test_crate_to_url() {
        let reg = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.0".into(),
        });
        assert_eq!(
            crate_to_url(&reg).unwrap(),
            "https://crates.io/crates/lazy_static/1.0".to_string()
        );

        let repo = GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
            sha: None,
        };
        let gh = Crate::GitHub(repo.clone());

        assert_eq!(
            crate_to_url(&gh).unwrap(),
            "https://github.com/brson/hello-rs".to_string()
        );

        let repo = GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
            sha: Some("f00".into()),
        };
        let gh = Crate::GitHub(repo);
        assert_eq!(
            crate_to_url(&gh).unwrap(),
            "https://github.com/brson/hello-rs/tree/f00".to_string()
        );
    }

    #[test]
    fn test_compare() {
        use crate::results::{FailureReason::*, TestResult::*};

        macro_rules! test_compare {
            ($cmp:ident, $config:expr, $reg:expr, [$($a:expr, $b:expr => $c:ident;)*]) => {
                $(
                    assert_eq!(
                        $cmp(
                            $config,
                            $reg,
                            Some(&$a),
                            Some(&$b),
                        ),
                        Comparison::$c
                    );
                )*
            }
        }

        let mut config = Config::default();
        let reg = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.0".into(),
        });

        test_compare!(
            compare,
            &config,
            &reg,
            [
                BuildFail(Unknown), BuildFail(Unknown) => SameBuildFail;
                TestFail(Unknown), TestFail(Unknown) => SameTestFail;
                TestSkipped, TestSkipped => SameTestSkipped;
                TestPass, TestPass => SameTestPass;

                // Non-spurious fixes/regressions
                BuildFail(Unknown), TestFail(Unknown) => Fixed;
                BuildFail(Unknown), TestSkipped => Fixed;
                BuildFail(Unknown), TestPass => Fixed;
                TestFail(Unknown), TestPass => Fixed;
                TestPass, TestFail(Unknown) => Regressed;
                TestPass, BuildFail(Unknown) => Regressed;
                TestSkipped, BuildFail(Unknown) => Regressed;
                TestFail(Unknown), BuildFail(Unknown) => Regressed;

                // ICE is special
                BuildFail(Unknown), BuildFail(ICE) => Regressed;
                BuildFail(OOM), BuildFail(ICE) => Regressed;
                BuildFail(ICE), BuildFail(ICE) => SameBuildFail;

                // Spurious fixes/regressions
                BuildFail(OOM), TestFail(Unknown) => SpuriousFixed;
                BuildFail(Unknown), TestFail(OOM) => SpuriousFixed;
                BuildFail(OOM), TestSkipped => SpuriousFixed;
                BuildFail(OOM), TestPass => SpuriousFixed;
                TestFail(OOM), TestPass => SpuriousFixed;
                TestPass, TestFail(OOM) => SpuriousRegressed;
                TestPass, BuildFail(OOM) => SpuriousRegressed;
                TestSkipped, BuildFail(OOM) => SpuriousRegressed;
                TestFail(OOM), BuildFail(Unknown) => SpuriousRegressed;
                TestFail(Unknown), BuildFail(OOM) => SpuriousRegressed;

                // Errors
                Error, TestPass => Error;
                Error, TestSkipped => Error;
                Error, TestFail(Unknown) => Error;
                Error, BuildFail(Unknown) => Error;
                TestPass, Error => Error;
                TestSkipped, Error => Error;
                TestFail(Unknown), Error => Error;
                BuildFail(Unknown), Error => Error;

                // Skipped
                Skipped, Skipped => Skipped;
                Skipped, TestPass => Skipped;
                Skipped, TestSkipped => Skipped;
                Skipped, TestFail(Unknown) => Skipped;
                Skipped, BuildFail(Unknown) => Skipped;
                TestPass, Skipped => Skipped;
                TestSkipped, Skipped => Skipped;
                TestFail(Unknown), Skipped => Skipped;
                BuildFail(Unknown), Skipped => Skipped;


                // Broken
                BrokenCrate(BrokenReason::Unknown), TestPass => Broken;
                BrokenCrate(BrokenReason::Unknown), TestSkipped => Broken;
                BrokenCrate(BrokenReason::Unknown), TestFail(Unknown) => Broken;
                BrokenCrate(BrokenReason::Unknown), BuildFail(Unknown) => Broken;
                TestPass, BrokenCrate(BrokenReason::Unknown) => Broken;
                TestSkipped, BrokenCrate(BrokenReason::Unknown) => Broken;
                TestFail(Unknown), BrokenCrate(BrokenReason::Unknown) => Broken;
                BuildFail(Unknown), BrokenCrate(BrokenReason::Unknown) => Broken;
            ]
        );

        assert_eq!(compare(&config, &reg, None, None), Comparison::Unknown);

        config.crates.insert(
            "lazy_static".into(),
            CrateConfig {
                skip: true,
                skip_tests: false,
                quiet: false,
                broken: false,
            },
        );
        assert_eq!(compare(&config, &reg, None, None), Comparison::Skipped);
    }

    #[test]
    fn test_report_generation() {
        let config = Config::default();

        let repo = GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
            sha: Some("f00".into()),
        };
        let gh = Crate::GitHub(repo.clone());
        let reg = Crate::Registry(RegistryCrate {
            name: "syn".into(),
            version: "1.0.0".into(),
        });

        let ex = Experiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            cap_lints: CapLints::Forbid,
            priority: 0,
            created_at: ::chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            github_issue: None,
            status: Status::GeneratingReport,
            assigned_to: None,
            report_url: None,
            ignore_blacklist: false,
            requirement: None,
        };

        let mut db = DummyDB::default();
        db.add_dummy_result(
            &ex,
            gh.clone(),
            MAIN_TOOLCHAIN.clone(),
            TestResult::TestPass,
        );
        db.add_dummy_result(
            &ex,
            gh.clone(),
            TEST_TOOLCHAIN.clone(),
            TestResult::BuildFail(FailureReason::Unknown),
        );
        db.add_dummy_log(
            &ex,
            gh.clone(),
            MAIN_TOOLCHAIN.clone(),
            EncodedLog::Plain(b"stable log".to_vec()),
        );
        db.add_dummy_log(
            &ex,
            gh.clone(),
            TEST_TOOLCHAIN.clone(),
            EncodedLog::Plain(b"beta log".to_vec()),
        );

        db.add_dummy_result(
            &ex,
            reg.clone(),
            MAIN_TOOLCHAIN.clone(),
            TestResult::TestPass,
        );
        db.add_dummy_result(
            &ex,
            reg.clone(),
            TEST_TOOLCHAIN.clone(),
            TestResult::BuildFail(FailureReason::Unknown),
        );
        db.add_dummy_log(
            &ex,
            reg.clone(),
            MAIN_TOOLCHAIN.clone(),
            EncodedLog::Plain(b"stable log".to_vec()),
        );
        db.add_dummy_log(
            &ex,
            reg.clone(),
            TEST_TOOLCHAIN.clone(),
            EncodedLog::Plain(b"beta log".to_vec()),
        );

        let writer = DummyWriter::default();
        gen(&db, &ex, &[gh, reg], &writer, &config, false).unwrap();

        assert_eq!(
            writer.get("config.json", &mime::APPLICATION_JSON),
            serde_json::to_vec(&ex).unwrap()
        );

        assert_eq!(
            &writer.get("stable/gh/brson.hello-rs/log.txt", &mime::TEXT_PLAIN_UTF_8),
            b"stable log"
        );
        assert_eq!(
            &writer.get("beta/gh/brson.hello-rs/log.txt", &mime::TEXT_PLAIN_UTF_8),
            b"beta log"
        );

        let result: RawTestResults =
            serde_json::from_slice(&writer.get("results.json", &mime::APPLICATION_JSON)).unwrap();

        assert_eq!(result.crates.len(), 2);
        let gh_result = &result.crates[0];
        let reg_result = &result.crates[1];

        assert_eq!(gh_result.name.as_str(), "brson.hello-rs.f00");
        assert_eq!(
            gh_result.url.as_str(),
            "https://github.com/brson/hello-rs/tree/f00"
        );
        assert_eq!(gh_result.res, Comparison::Regressed);
        assert_eq!(
            (&gh_result.runs[0]).as_ref().unwrap().res,
            TestResult::TestPass
        );
        assert_eq!(
            (&gh_result.runs[1]).as_ref().unwrap().res,
            TestResult::BuildFail(FailureReason::Unknown)
        );
        assert_eq!(
            (&gh_result.runs[0]).as_ref().unwrap().log.as_str(),
            "stable/gh/brson.hello-rs"
        );
        assert_eq!(
            (&gh_result.runs[1]).as_ref().unwrap().log.as_str(),
            "beta/gh/brson.hello-rs"
        );

        assert_eq!(reg_result.name.as_str(), "syn-1.0.0");
        assert_eq!(
            reg_result.url.as_str(),
            "https://crates.io/crates/syn/1.0.0"
        );
        assert_eq!(reg_result.res, Comparison::Regressed);
        assert_eq!(
            (&reg_result.runs[0]).as_ref().unwrap().res,
            TestResult::TestPass
        );
        assert_eq!(
            (&reg_result.runs[1]).as_ref().unwrap().res,
            TestResult::BuildFail(FailureReason::Unknown)
        );
        assert_eq!(
            (&reg_result.runs[0]).as_ref().unwrap().log.as_str(),
            "stable/reg/syn-1.0.0"
        );
        assert_eq!(
            (&reg_result.runs[1]).as_ref().unwrap().log.as_str(),
            "beta/reg/syn-1.0.0"
        );

        assert_eq!(
            writer.get("retry-regressed-list.txt", &mime::TEXT_PLAIN_UTF_8),
            b"brson/hello-rs\nsyn\n",
        );
    }
}
