use errors::*;
use ex;
use file;
use handlebars::Handlebars;
use mime::{self, Mime};
use results::{CrateResultWriter, ExperimentResultDB, FileDB, TestResult};
use serde_json;
use std::{fs, io};
use std::borrow::Cow;
use std::convert::AsRef;
use std::fmt::{self, Display};
use std::fs::File;
use std::path::{Path, PathBuf};

mod s3;
pub use self::s3::{S3Prefix, S3Writer};

#[derive(Serialize, Deserialize)]
pub struct TestResults {
    crates: Vec<CrateResult>,
}

#[derive(Serialize, Deserialize)]
struct CrateResult {
    name: String,
    url: String,
    res: Comparison,
    runs: [Option<BuildTestResult>; 2],
}

#[derive(Serialize, Deserialize)]
enum Comparison {
    Regressed,
    Fixed,
    Unknown,
    SameBuildFail,
    SameTestFail,
    SameTestPass,
}

#[derive(Serialize, Deserialize)]
struct BuildTestResult {
    res: TestResult,
    log: String,
}

pub fn generate_report(ex: &ex::Experiment) -> Result<TestResults> {
    let db = FileDB::for_experiment(ex);
    assert_eq!(ex.toolchains.len(), 2);

    let res = ex.crates()?
        .into_iter()
        .map(|krate| {
            // Any errors here will turn into unknown results
            let crate_results = ex.toolchains.iter().map(|tc| -> Result<BuildTestResult> {
                let writer = db.for_crate(&krate, tc);
                let res = writer.load_test_result()?;
                // If there was no test result return an error
                let res = res.ok_or_else(|| Error::from("no result"))?;
                let rel_log = writer.result_path_fragement();

                Ok(BuildTestResult {
                    res: res,
                    log: format!("{}", rel_log.display()),
                })
            });
            // Convert errors to Nones
            let mut crate_results = crate_results.map(|r| r.ok()).collect::<Vec<_>>();
            let crate2 = crate_results.pop().expect("");
            let crate1 = crate_results.pop().expect("");
            let comp = compare(&crate1, &crate2);

            CrateResult {
                name: crate_to_name(&krate),
                url: crate_to_url(&krate),
                res: comp,
                runs: [crate1, crate2],
            }
        })
        .collect::<Vec<_>>();

    Ok(TestResults { crates: res })
}

const PROGRESS_FRACTION: usize = 10; // write progress every ~1/N crates

fn write_logs<W: ReportWriter>(ex: &ex::Experiment, dest: &W) -> Result<()> {
    let db = FileDB::for_experiment(ex);
    let crates = ex.crates()?;
    let num_crates = crates.len();
    let progress_every = (num_crates / PROGRESS_FRACTION) + 1;
    for (i, krate) in crates.into_iter().enumerate() {
        if i % progress_every == 0 {
            info!("wrote logs for {}/{} crates", i, num_crates)
        }
        for tc in &ex.toolchains {
            let writer = db.for_crate(&krate, tc);
            let rel_log = writer.result_path_fragement();

            match writer.read_log() {
                Ok(ref mut result_log) => {
                    dest.copy(result_log, rel_log.join("log.txt"), &mime::TEXT_PLAIN_UTF_8)?
                }
                Err(e) => error!{"Could not read log for {} {}: {}", krate, tc.to_string(), e},
            }
        }
    }
    Ok(())
}

pub fn gen<W: ReportWriter + Display>(ex_name: &str, dest: &W) -> Result<()> {
    let ex = ex::Experiment::load(ex_name)?;

    let res = generate_report(&ex)?;
    let shas = ex.load_shas()?;

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
    dest.write_string(
        "shas.json",
        serde_json::to_string(&shas)?.into(),
        &mime::APPLICATION_JSON,
    )?;

    info!("writing html files");
    write_html_files(dest)?;
    info!("writing logs");
    write_logs(&ex, dest)?;

    Ok(())
}

fn crate_to_name(c: &ex::ExCrate) -> String {
    match *c {
        ex::ExCrate::Version {
            ref name,
            ref version,
        } => format!("{}-{}", name, version),
        ex::ExCrate::Repo {
            ref org,
            ref name,
            ref sha,
        } => format!("{}.{}.{}", org, name, sha),
    }
}

fn crate_to_url(c: &ex::ExCrate) -> String {
    match *c {
        ex::ExCrate::Version {
            ref name,
            ref version,
        } => format!("https://crates.io/crates/{}/{}", name, version),
        ex::ExCrate::Repo {
            ref org,
            ref name,
            ref sha,
        } => format!("https://github.com/{}/{}/tree/{}", org, name, sha),
    }
}

fn compare(r1: &Option<BuildTestResult>, r2: &Option<BuildTestResult>) -> Comparison {
    use results::TestResult::*;
    match (r1, r2) {
        (
            &Some(BuildTestResult { res: ref res1, .. }),
            &Some(BuildTestResult { res: ref res2, .. }),
        ) => match (res1, res2) {
            (&BuildFail, &BuildFail) => Comparison::SameBuildFail,
            (&TestFail, &TestFail) => Comparison::SameTestFail,
            (&TestPass, &TestPass) => Comparison::SameTestPass,
            (&BuildFail, &TestFail) | (&BuildFail, &TestPass) | (&TestFail, &TestPass) => {
                Comparison::Fixed
            }
            (&TestPass, &TestFail) | (&TestPass, &BuildFail) | (&TestFail, &BuildFail) => {
                Comparison::Regressed
            }
        },
        _ => Comparison::Unknown,
    }
}

#[derive(Serialize, Deserialize)]
pub struct Context {
    pub config_url: String,
    pub results_url: String,
    pub static_url: String,
}

fn write_html_files<W: ReportWriter>(dest: &W) -> Result<()> {
    let html_in = include_str!("../../template/report.html");
    let js_in = include_str!("../../static/report.js");
    let css_in = include_str!("../../static/report.css");
    let html_out = "index.html";
    let js_out = "report.js";
    let css_out = "report.css";

    let context = Context {
        config_url: "config.json".into(),
        results_url: "results.json".into(),
        static_url: "".into(),
    };
    let html = Handlebars::new()
        .template_render(html_in, &context)
        .chain_err(|| "Couldn't render template")?;

    dest.write_string(&html_out, html.into(), &mime::TEXT_HTML)?;
    dest.write_string(&js_out, js_in.into(), &mime::TEXT_JAVASCRIPT)?;
    dest.write_string(&css_out, css_in.into(), &mime::TEXT_CSS)?;

    Ok(())
}

pub trait ReportWriter {
    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, mime: &Mime) -> Result<()>;
    fn copy<P: AsRef<Path>, R: io::Read>(&self, r: &mut R, path: P, mime: &Mime) -> Result<()>;
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
    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, _: &Mime) -> Result<()> {
        self.create_prefix(path.as_ref())?;
        file::write_string(&self.0.join(path.as_ref()), s.as_ref())
    }
    fn copy<P: AsRef<Path>, R: io::Read>(&self, r: &mut R, path: P, _: &Mime) -> Result<()> {
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
