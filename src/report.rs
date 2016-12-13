use gh_mirrors;
use std::path::{Path, PathBuf};
use toolchain;
use errors::*;
use ex;
use ex_run;
use serde_json;
use file;

fn results_file(ex_name: &str) -> PathBuf {
    ex::ex_dir(ex_name).join("results.json")
}

#[derive(Serialize, Deserialize)]
struct TestResults {
    crates: Vec<CrateResult>
}

#[derive(Serialize, Deserialize)]
struct CrateResult {
    name: String,
    res: Comparison,
    runs: [Option<BuildTestResult>; 2]
}

#[derive(Serialize, Deserialize)]
enum Comparison {
    Regressed,
    Fixed,
    SameFail,
    SameBuildPass,
    SameTestPass,
    Unknown,
}

#[derive(Serialize, Deserialize)]
struct BuildTestResult {
    res: ex_run::TestResult,
    log: String,
}

pub fn gen(ex_name: &str) -> Result<()> {
    let config = ex::load_config(ex_name)?;
    assert!(config.toolchains.len() == 2);

    let ex_dir = ex::ex_dir(ex_name);

    let res = ex::ex_crates_and_dirs(ex_name)?.into_iter().map(|(krate, _)| {
        // Any errors here will turn into unknown results
        let crate_results = config.toolchains.iter().map(|tc| -> Result<BuildTestResult> {
            let ref tcs = toolchain::tc_to_string(tc);
            let res = ex_run::get_test_result(ex_name, &krate, tcs)?;
            // If there was no test result return an error
            let res = res.ok_or_else(|| Error::from("no result"))?;
            let result_file = ex_run::result_file(ex_name, &krate, tcs)?;
            let result_log = ex_run::result_log(ex_name, &krate, tcs)?;
            let rel_log = relative(&ex_dir, &result_log)?;

            Ok(BuildTestResult {
                res: res,
                log: format!("{}", rel_log.display())
            })
        });
        // Convert errors to Nones
        let mut crate_results = crate_results.map(|r| r.ok()).collect::<Vec<_>>();
        let crate2 = crate_results.pop().expect("");
        let crate1 = crate_results.pop().expect("");
        let comp = compare(&crate1, &crate2);

        CrateResult {
            name: crate_to_name(&krate).unwrap_or("<unknown>".into()),
            res: comp,
            runs: [crate1, crate2]
        }
    }).collect::<Vec<_>>();

    let res = TestResults {
        crates: res
    };

    let json = serde_json::to_string(&res)?;
    log!("writing results to {}", results_file(ex_name).display());
    file::write_string(&results_file(ex_name), &json)?;

    write_html_files(&ex_dir)?;

    Ok(())
}

fn crate_to_name(c: &ex::ExCrate) -> Result<String> {
    match *c {
        ex::ExCrate::Version(ref n, ref v) => Ok(format!("{}-{}", n, v)),
        ex::ExCrate::Repo(ref url, ref sha) => {
            let (org, name) = gh_mirrors::gh_url_to_org_and_name(url)?;
            Ok(format!("{}.{}.{}", org, name, sha))
        }
    }
}

fn relative(parent: &Path, child: &Path) -> Result<PathBuf> {
    Ok(child.strip_prefix(parent)
       .chain_err(|| "calculating relative log file")?.into())
}

fn compare(r1: &Option<BuildTestResult>, r2: &Option<BuildTestResult>) -> Comparison {
    use ex_run::TestResult::*;
    match (r1, r2) {
        (&Some(BuildTestResult { res: ref res1, .. }),
         &Some(BuildTestResult { res: ref res2, .. })) => {
            match (res1, res2) {
                (&Fail, &Fail) => Comparison::SameFail,
                (&BuildPass, &BuildPass) => Comparison::SameBuildPass,
                (&TestPass, &TestPass) => Comparison::SameTestPass,
                (&Fail, &BuildPass) => Comparison::Fixed,
                (&Fail, &TestPass) => Comparison::Fixed,
                (&BuildPass, &TestPass) => Comparison::Fixed,
                (&TestPass, &BuildPass) => Comparison::Regressed,
                (&TestPass, &Fail) => Comparison::Regressed,
                (&BuildPass, &Fail) => Comparison::Regressed,
            }
        }
        _ => Comparison::Unknown
    }
}

fn write_html_files(dir: &Path) -> Result<()> {
    let html_in = include_str!("report.html");
    let js_in = include_str!("report.js");
    let css_in = include_str!("report.css");
    let html_out = dir.join("index.html");
    let js_out = dir.join("report.js");
    let css_out = dir.join("report.css");

    log!("writing report to {}", html_out.display());

    file::write_string(&html_out, html_in)?;
    file::write_string(&js_out, js_in)?;
    file::write_string(&css_out, css_in)?;

    Ok(())
}
