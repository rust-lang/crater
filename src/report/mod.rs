use config::Config;
use crates::{Crate, GitHubRepo};
use errors::*;
use experiments::Experiment;
use mime::{self, Mime};
use results::{ReadResults, TestResult};
use serde_json;
use std::borrow::Cow;
#[cfg(test)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::AsRef;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use toolchain::Toolchain;
use url::percent_encoding::{utf8_percent_encode, DEFAULT_ENCODE_SET};
use utils;

mod archives;
mod html;
mod s3;

pub use self::s3::{get_client_for_bucket, S3Prefix, S3Writer};

define_encode_set! {
    pub REPORT_ENCODE_SET = [DEFAULT_ENCODE_SET] | { '+' }
}

#[inline]
fn url_encode(input: &str) -> String {
    utf8_percent_encode(input, REPORT_ENCODE_SET).to_string()
}

#[derive(Serialize, Deserialize)]
pub struct TestResults {
    crates: Vec<CrateResult>,
}

#[derive(Serialize, Deserialize, Clone)]
struct CrateResult {
    name: String,
    url: String,
    res: Comparison,
    runs: [Option<BuildTestResult>; 2],
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Copy, Clone, Debug)]
enum Comparison {
    Regressed,
    Fixed,
    Skipped,
    Unknown,
    Error,
    SameBuildFail,
    SameTestFail,
    SameTestSkipped,
    SameTestPass,
}

impl Comparison {
    fn show_in_summary(self) -> bool {
        match self {
            Comparison::Regressed | Comparison::Fixed | Comparison::Unknown | Comparison::Error => {
                true
            }
            Comparison::Skipped
            | Comparison::SameBuildFail
            | Comparison::SameTestFail
            | Comparison::SameTestSkipped
            | Comparison::SameTestPass => false,
        }
    }
}

impl Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Comparison::Regressed => "regressed",
                Comparison::Fixed => "fixed",
                Comparison::Skipped => "skipped",
                Comparison::Unknown => "unknown",
                Comparison::Error => "error",
                Comparison::SameBuildFail => "build-fail",
                Comparison::SameTestFail => "test-fail",
                Comparison::SameTestSkipped => "test-skipped",
                Comparison::SameTestPass => "test-pass",
            }
        )
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct BuildTestResult {
    res: TestResult,
    log: String,
}

fn crate_to_path_fragment(toolchain: &Toolchain, krate: &Crate, encode: bool) -> PathBuf {
    let mut path = PathBuf::new();
    if encode {
        path.push(url_encode(&toolchain.to_string()));
    } else {
        path.push(toolchain.to_string());
    }

    match *krate {
        Crate::Registry(ref details) => {
            path.push("reg");

            let name = format!("{}-{}", details.name, details.version);
            if encode {
                path.push(url_encode(&name));
            } else {
                path.push(name);
            }
        }
        Crate::GitHub(ref repo) => {
            path.push("gh");

            let name = format!("{}.{}", repo.org, repo.name);
            if encode {
                path.push(url_encode(&name));
            } else {
                path.push(name);
            }
        }
    }

    path
}

pub fn generate_report<DB: ReadResults>(
    db: &DB,
    config: &Config,
    ex: &Experiment,
) -> Result<TestResults> {
    let shas = db.load_all_shas(ex)?;
    let res = ex
        .crates
        .clone()
        .into_iter()
        .map(|krate| {
            // Any errors here will turn into unknown results
            let crate_results = ex.toolchains.iter().map(|tc| -> Result<BuildTestResult> {
                let res = db
                    .load_test_result(ex, tc, &krate)?
                    .ok_or_else(|| "no result")?;

                Ok(BuildTestResult {
                    res,
                    log: crate_to_path_fragment(tc, &krate, true)
                        .to_str()
                        .unwrap()
                        .to_string(),
                })
            });
            // Convert errors to Nones
            let mut crate_results = crate_results.map(|r| r.ok()).collect::<Vec<_>>();
            let crate2 = crate_results.pop().unwrap();
            let crate1 = crate_results.pop().unwrap();
            let comp = compare(
                config,
                &krate,
                crate1.as_ref().map(|b| b.res),
                crate2.as_ref().map(|b| b.res),
            );

            Ok(CrateResult {
                name: crate_to_name(&krate, &shas)?,
                url: crate_to_url(&krate, &shas)?,
                res: comp,
                runs: [crate1, crate2],
            })
        }).collect::<Result<Vec<_>>>()?;

    Ok(TestResults { crates: res })
}

const PROGRESS_FRACTION: usize = 10; // write progress every ~1/N crates

fn write_logs<DB: ReadResults, W: ReportWriter>(
    db: &DB,
    ex: &Experiment,
    dest: &W,
    config: &Config,
) -> Result<()> {
    let num_crates = ex.crates.len();
    let progress_every = (num_crates / PROGRESS_FRACTION) + 1;
    for (i, krate) in ex.crates.iter().enumerate() {
        if i % progress_every == 0 {
            info!("wrote logs for {}/{} crates", i, num_crates)
        }

        if config.should_skip(krate) {
            continue;
        }

        for tc in &ex.toolchains {
            let log_path = crate_to_path_fragment(tc, krate, false).join("log.txt");
            let content = db
                .load_log(ex, tc, krate)
                .and_then(|c| c.ok_or_else(|| "missing logs".into()))
                .chain_err(|| format!("failed to read log of {} on {}", krate, tc.to_string()));
            let content = match content {
                Ok(c) => c,
                Err(e) => {
                    utils::report_error(&e);
                    continue;
                }
            };
            dest.write_bytes(log_path, content, &mime::TEXT_PLAIN_UTF_8)?;
        }
    }
    Ok(())
}

pub fn gen<DB: ReadResults, W: ReportWriter + Display>(
    db: &DB,
    ex: &Experiment,
    dest: &W,
    config: &Config,
) -> Result<()> {
    let res = generate_report(db, config, ex)?;

    info!("writing results to {}", dest);
    info!("writing metadata");
    dest.write_string(
        "results.json",
        serde_json::to_string(&res)?.into(),
        &mime::APPLICATION_JSON,
    )?;
    dest.write_string(
        "config.json",
        serde_json::to_string(&ex)?.into(),
        &mime::APPLICATION_JSON,
    )?;

    info!("writing archives");
    let available_archives = archives::write_logs_archives(db, ex, dest, config)?;
    info!("writing html files");
    html::write_html_report(ex, &res, available_archives, dest)?;
    info!("writing logs");
    write_logs(db, ex, dest, config)?;

    Ok(())
}

fn crate_to_name(c: &Crate, shas: &HashMap<GitHubRepo, String>) -> Result<String> {
    Ok(match *c {
        Crate::Registry(ref details) => format!("{}-{}", details.name, details.version),
        Crate::GitHub(ref repo) => {
            if let Some(sha) = shas.get(repo) {
                format!("{}.{}.{}", repo.org, repo.name, sha)
            } else {
                format!("{}.{}", repo.org, repo.name)
            }
        }
    })
}

fn crate_to_url(c: &Crate, shas: &HashMap<GitHubRepo, String>) -> Result<String> {
    Ok(match *c {
        Crate::Registry(ref details) => format!(
            "https://crates.io/crates/{}/{}",
            details.name, details.version
        ),
        Crate::GitHub(ref repo) => {
            if let Some(sha) = shas.get(repo) {
                format!("https://github.com/{}/{}/tree/{}", repo.org, repo.name, sha)
            } else {
                format!("https://github.com/{}/{}", repo.org, repo.name)
            }
        }
    })
}

fn compare(
    config: &Config,
    krate: &Crate,
    r1: Option<TestResult>,
    r2: Option<TestResult>,
) -> Comparison {
    use results::TestResult::*;
    match (r1, r2) {
        (Some(res1), Some(res2)) => match (res1, res2) {
            (BuildFail, BuildFail) => Comparison::SameBuildFail,
            (TestFail, TestFail) => Comparison::SameTestFail,
            (TestSkipped, TestSkipped) => Comparison::SameTestSkipped,
            (TestPass, TestPass) => Comparison::SameTestPass,
            (BuildFail, TestFail)
            | (BuildFail, TestSkipped)
            | (BuildFail, TestPass)
            | (TestFail, TestPass) => Comparison::Fixed,
            (TestPass, TestFail)
            | (TestPass, BuildFail)
            | (TestSkipped, BuildFail)
            | (TestFail, BuildFail) => Comparison::Regressed,
            (Error, _) | (_, Error) => Comparison::Error,
            (TestFail, TestSkipped)
            | (TestPass, TestSkipped)
            | (TestSkipped, TestFail)
            | (TestSkipped, TestPass) => {
                panic!("can't compare {} and {}", res1, res2);
            }
        },
        _ if config.should_skip(krate) => Comparison::Skipped,
        _ => Comparison::Unknown,
    }
}

pub trait ReportWriter {
    fn write_bytes<P: AsRef<Path>>(&self, path: P, b: Vec<u8>, mime: &Mime) -> Result<()>;
    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, mime: &Mime) -> Result<()>;
    fn copy<P: AsRef<Path>, R: Read>(&self, r: &mut R, path: P, mime: &Mime) -> Result<()>;
}

pub struct FileWriter(PathBuf);

impl FileWriter {
    pub fn create(dest: PathBuf) -> Result<FileWriter> {
        fs::create_dir_all(&dest)?;
        Ok(FileWriter(dest))
    }
    fn create_prefix(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(self.0.join(parent))?;
        }
        Ok(())
    }
}

impl ReportWriter for FileWriter {
    fn write_bytes<P: AsRef<Path>>(&self, path: P, b: Vec<u8>, _: &Mime) -> Result<()> {
        self.create_prefix(path.as_ref())?;
        fs::write(&self.0.join(path.as_ref()), &b)?;
        Ok(())
    }

    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, _: &Mime) -> Result<()> {
        self.create_prefix(path.as_ref())?;
        fs::write(&self.0.join(path.as_ref()), s.as_ref().as_bytes())?;
        Ok(())
    }

    fn copy<P: AsRef<Path>, R: Read>(&self, r: &mut R, path: P, _: &Mime) -> Result<()> {
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
    fn write_bytes<P: AsRef<Path>>(&self, path: P, b: Vec<u8>, mime: &Mime) -> Result<()> {
        self.results
            .borrow_mut()
            .insert((path.as_ref().to_path_buf(), mime.clone()), b);
        Ok(())
    }

    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, mime: &Mime) -> Result<()> {
        self.results.borrow_mut().insert(
            (path.as_ref().to_path_buf(), mime.clone()),
            s.bytes().collect(),
        );
        Ok(())
    }

    fn copy<P: AsRef<Path>, R: Read>(&self, r: &mut R, path: P, mime: &Mime) -> Result<()> {
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
    use config::{Config, CrateConfig};
    use crates::{Crate, GitHubRepo, RegistryCrate};
    use experiments::{CapLints, Experiment, Mode, Status};
    use results::{DummyDB, TestResult};
    use std::collections::HashMap;
    use toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    #[test]
    fn test_crate_to_path_fragment() {
        let reg = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.0".into(),
        });
        let gh = Crate::GitHub(GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
        });
        let plus = Crate::Registry(RegistryCrate {
            name: "foo".into(),
            version: "1.0+bar".into(),
        });

        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &reg, false),
            PathBuf::from("stable/reg/lazy_static-1.0")
        );
        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &gh, false),
            PathBuf::from("stable/gh/brson.hello-rs")
        );
        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &plus, false),
            PathBuf::from("stable/reg/foo-1.0+bar")
        );
        assert_eq!(
            crate_to_path_fragment(&MAIN_TOOLCHAIN, &plus, true),
            PathBuf::from("stable/reg/foo-1.0%2Bbar")
        );
    }

    #[test]
    fn test_crate_to_name() {
        let reg = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.0".into(),
        });
        let repo = GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
        };
        let gh = Crate::GitHub(repo.clone());

        let mut shas = HashMap::new();
        shas.insert(repo, "f00".into());

        assert_eq!(
            crate_to_name(&reg, &shas).unwrap(),
            "lazy_static-1.0".to_string()
        );
        assert_eq!(
            crate_to_name(&gh, &shas).unwrap(),
            "brson.hello-rs.f00".to_string()
        );
    }

    #[test]
    fn test_crate_to_url() {
        let reg = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.0".into(),
        });
        let repo = GitHubRepo {
            org: "brson".into(),
            name: "hello-rs".into(),
        };
        let gh = Crate::GitHub(repo.clone());

        let mut shas = HashMap::new();
        shas.insert(repo, "f00".into());

        assert_eq!(
            crate_to_url(&reg, &shas).unwrap(),
            "https://crates.io/crates/lazy_static/1.0".to_string()
        );
        assert_eq!(
            crate_to_url(&gh, &shas).unwrap(),
            "https://github.com/brson/hello-rs/tree/f00".to_string()
        );
    }

    #[test]
    fn test_compare() {
        macro_rules! test_compare {
            ($cmp:ident, $config:expr, $reg:expr, [$($a:ident + $b:ident = $c:ident,)*]) => {
                $(
                    assert_eq!(
                        $cmp(
                            $config,
                            $reg,
                            Some(TestResult::$a),
                            Some(TestResult::$b),
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
                BuildFail + BuildFail = SameBuildFail,
                TestFail + TestFail = SameTestFail,
                TestSkipped + TestSkipped = SameTestSkipped,
                TestPass + TestPass = SameTestPass,
                BuildFail + TestFail = Fixed,
                BuildFail + TestSkipped = Fixed,
                BuildFail + TestPass = Fixed,
                TestFail + TestPass = Fixed,
                TestPass + TestFail = Regressed,
                TestPass + BuildFail = Regressed,
                TestSkipped + BuildFail = Regressed,
                TestFail + BuildFail = Regressed,
                Error + TestPass = Error,
                Error + TestSkipped = Error,
                Error + TestFail = Error,
                Error + BuildFail = Error,
                TestPass + Error = Error,
                TestSkipped + Error = Error,
                TestFail + Error = Error,
                BuildFail + Error = Error,
            ]
        );

        assert_eq!(compare(&config, &reg, None, None), Comparison::Unknown);

        config.crates.insert(
            "lazy_static".into(),
            CrateConfig {
                skip: true,
                skip_tests: false,
                quiet: false,
                update_lockfile: false,
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
        };
        let gh = Crate::GitHub(repo.clone());

        let ex = Experiment {
            name: "foo".to_string(),
            crates: vec![gh.clone()],
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
        };

        let mut db = DummyDB::default();
        db.add_dummy_sha(&ex, repo.clone(), "f00".to_string());
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
            TestResult::BuildFail,
        );
        db.add_dummy_log(
            &ex,
            gh.clone(),
            MAIN_TOOLCHAIN.clone(),
            b"stable log".to_vec(),
        );
        db.add_dummy_log(
            &ex,
            gh.clone(),
            TEST_TOOLCHAIN.clone(),
            b"beta log".to_vec(),
        );

        let writer = DummyWriter::default();
        gen(&db, &ex, &writer, &config).unwrap();

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

        let result: TestResults =
            serde_json::from_slice(&writer.get("results.json", &mime::APPLICATION_JSON)).unwrap();

        assert_eq!(result.crates.len(), 1);
        let crate_result = &result.crates[0];

        assert_eq!(crate_result.name.as_str(), "brson.hello-rs.f00");
        assert_eq!(
            crate_result.url.as_str(),
            "https://github.com/brson/hello-rs/tree/f00"
        );
        assert_eq!(crate_result.res, Comparison::Regressed);
        assert_eq!(
            (&crate_result.runs[0]).as_ref().unwrap().res,
            TestResult::TestPass
        );
        assert_eq!(
            (&crate_result.runs[1]).as_ref().unwrap().res,
            TestResult::BuildFail
        );
        assert_eq!(
            (&crate_result.runs[0]).as_ref().unwrap().log.as_str(),
            "stable/gh/brson.hello-rs"
        );
        assert_eq!(
            (&crate_result.runs[1]).as_ref().unwrap().log.as_str(),
            "beta/gh/brson.hello-rs"
        );
    }
}
