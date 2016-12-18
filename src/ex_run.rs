use gh_mirrors;
use std::time::Instant;
use RUSTUP_HOME;
use CARGO_HOME;
use std::env;
use std::fs;
use errors::*;
use EXPERIMENT_DIR;
use std::path::{Path, PathBuf};
use crates;
use lists::{self, Crate};
use run;
use std::collections::{HashMap, HashSet};
use serde_json;
use file;
use toolchain::{self, Toolchain};
use util;
use std::fmt::{self, Formatter, Display};
use log;
use toml_frobber;
use TEST_DIR;
use ex::*;

pub fn result_dir(ex_name: &str, c: &ExCrate, toolchain: &str) -> Result<PathBuf> {
    let tc = toolchain::rustup_toolchain_name(toolchain)?;
    Ok(ex_dir(ex_name).join("res").join(tc).join(crate_to_dir(c)?))
}

pub fn result_file(ex_name: &str, c: &ExCrate, toolchain: &str) -> Result<PathBuf> {
    Ok(result_dir(ex_name, c, toolchain)?.join("results.txt"))
}

pub fn result_log(ex_name: &str, c: &ExCrate, toolchain: &str) -> Result<PathBuf> {
    Ok(result_dir(ex_name, c, toolchain)?.join("log.txt"))
}

fn crate_to_dir(c: &ExCrate) -> Result<String> {
    match *c {
        ExCrate::Version(ref n, ref v) => Ok(format!("reg/{}-{}", n, v)),
        ExCrate::Repo(ref url, ref sha) => {
            let (org, name) = gh_mirrors::gh_url_to_org_and_name(url)?;
            Ok(format!("gh/{}.{}.{}", org, name, sha))
        }
    }
}

pub fn run_build_and_test_test(ex_name: &str, toolchain: &str) -> Result<()> {
    run_test(ex_name, toolchain, build_and_test)
}

pub fn run_unstable_features(ex_name: &str, toolchain: &str) -> Result<()> {
    run_test(ex_name, toolchain, find_unstable_features)
}

pub fn run_test<F>(ex_name: &str, toolchain: &str, f: F) -> Result<()>
    where F: Fn(&str, &Path, &str) -> Result<TestResult>
{
    verify_toolchain(ex_name, toolchain)?;

    let crates = ex_crates_and_dirs(ex_name)?;

    // Just for reporting progress
    let total_crates = crates.len();
    let mut skipped_crates = 0;
    let mut completed_crates = 0;

    // These should add up to total_crates
    let mut sum_errors = 0;
    let mut sum_build_fail = 0;
    let mut sum_test_fail = 0;
    let mut sum_test_pass = 0;

    let start_time = Instant::now();

    log!("running {} tests", total_crates);
    for (ref c, ref dir) in crates {
        let r = {
            let existing_result = get_test_result(ex_name, c, toolchain)?;
            if let Some(r) = existing_result {
                skipped_crates += 1;

                log!("skipping crate {}. existing result: {}", c, r);
                log!("delete result file to rerun test: {}",
                     result_file(ex_name, c, toolchain)?.display());
                Ok(r)
            } else {
                completed_crates += 1;

                crates::with_work_crate(c, |path| {
                    with_frobbed_toml(ex_name, c, path)?;
                    with_captured_lockfile(ex_name, c, path)?;

                    run_single_test(ex_name, c, path, toolchain, &f)
                })
            }
        };

        match r {
            Err(ref e) => {
                log_err!("error testing crate {}:  {}", c, e);
                util::report_error(e);
            }
            Ok(ref r) => {
                // FIXME: Should errors be recorded?
                record_test_result(ex_name, c, toolchain, *r);
            }
        }

        match r {
            Err(_) => {
                sum_errors += 1;
            }
            Ok(TestResult::BuildFail) => sum_build_fail += 1,
            Ok(TestResult::TestFail) => sum_test_fail +=1,
            Ok(TestResult::TestPass) => sum_test_pass += 1,
        }

        let elapsed = Instant::now().duration_since(start_time).as_secs();
        let seconds_per_test = if completed_crates > 0 {
            (elapsed as f64) / (completed_crates as f64)
        } else {
            0.0
        };
        let remaining_tests = total_crates - completed_crates - skipped_crates;
        let remaining_time = remaining_tests * seconds_per_test as usize;

        let remaining_time_str = if remaining_time < 60 * 8 {
            format!("{:0} seconds", remaining_time)
        } else if remaining_time < 60 * 60 * 8 {
            format!("{:0} minutes", remaining_time / 60)
        } else {
            format!("{:0} hours", remaining_time / 60 / 60)
        };

        log!("progress: {} / {}", completed_crates + skipped_crates, total_crates);
        log!("{} crates tested in {} s. {:.2} s/crate. {} crates remaining. ~{}",
             completed_crates, elapsed, seconds_per_test, remaining_tests, remaining_time_str);
        log!("results: {} build-fail / {} test-fail / {} test-pass / {} errors",
             sum_build_fail, sum_test_pass, sum_test_pass, sum_errors);
    }

    Ok(())
}

fn verify_toolchain(ex_name: &str, toolchain: &str) -> Result<()> {
    let tc = toolchain::parse_toolchain(toolchain)?;
    let config = load_config(ex_name)?;
    if !config.toolchains.contains(&tc) {
        bail!("toolchain {} not in experiment", toolchain);
    }

    Ok(())
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum TestResult {
    BuildFail,
    TestFail,
    TestPass,
}

impl Display for TestResult {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        self.to_string().fmt(f)
    }
}

impl TestResult {
    fn from_str(s: &str) -> Result<TestResult> {
        match s {
            "build-fail" => Ok(TestResult::BuildFail),
            "test-fail" => Ok(TestResult::TestFail),
            "test-pass" => Ok(TestResult::TestPass),
            _ => Err(format!("bogus test result: {}", s).into())
        }
    }
    fn to_string(&self) -> String {
        match *self {
            TestResult::BuildFail => "build-fail",
            TestResult::TestFail => "test-fail",
            TestResult::TestPass => "test-pass",
        }.to_string()
    }
}

fn run_single_test<F>(ex_name: &str, c: &ExCrate, path: &Path,
                      toolchain: &str, f: &F) -> Result<TestResult>
    where F: Fn(&str, &Path, &str) -> Result<TestResult>
{
    let result_dir = result_dir(ex_name, c, toolchain)?;
    if result_dir.exists() {
        util::remove_dir_all(&result_dir)?;
    }
    fs::create_dir_all(&result_dir)?;
    let log_file = result_log(ex_name, c, toolchain)?;

    log::redirect(&log_file, || {
        let tc = toolchain::rustup_toolchain_name(toolchain)?;
        f(ex_name, path, &tc)
    })
}

fn build_and_test(ex_name: &str, path: &Path, rustup_tc: &str) -> Result<TestResult> {
    let tc_arg = &format!("+{}", rustup_tc);
    let build_r = run_in_docker(ex_name, path, &["cargo", tc_arg, "build", "--frozen"]);
    let test_r;

    if build_r.is_ok() {
        test_r = Some(run_in_docker(ex_name, path, &["cargo", tc_arg, "test", "--frozen"]));
    } else {
        test_r = None;
    }

    Ok(match (build_r, test_r) {
        (Err(_), None) => TestResult::BuildFail,
        (Ok(_), Some(Err(_))) => TestResult::TestFail,
        (Ok(_), Some(Ok(_))) => TestResult::TestPass,
        (_, _) => unreachable!()
    })
}

fn run_in_docker(ex_name: &str, path: &Path, args: &[&str]) -> Result<()> {

    let test_dir=absolute(path);
    let cargo_home=absolute(Path::new(CARGO_HOME));
    let rustup_home=absolute(Path::new(RUSTUP_HOME));
    // This is configured as CARGO_TARGET_DIR by the docker container itself
    let target_dir=absolute(&toolchain::target_dir(ex_name));

    fs::create_dir_all(&test_dir);
    fs::create_dir_all(&cargo_home);
    fs::create_dir_all(&rustup_home);
    fs::create_dir_all(&target_dir);

    let test_mount = &format!("{}:/test", test_dir.display());
    // FIXME this should be read-only https://github.com/rust-lang/cargo/issues/3256
    let cargo_home_mount = &format!("{}:/cargo-home", cargo_home.display());
    let rustup_home_mount = &format!("{}:/rustup-home:ro", rustup_home.display());
    let target_mount = &format!("{}:/target", target_dir.display());

    let image_name = "cargobomb";

    let user_env = &format!("USER_ID={}", user_id());
    let cmd_env = &format!("CMD={}", args.join(" "));

    let mut args_ = vec!["run", "-i",
                         "--rm",
                         "-v", test_mount,
                         "-v", cargo_home_mount,
                         "-v", rustup_home_mount,
                         "-v", target_mount,
                         "-e", user_env,
                         "-e", cmd_env,
                         image_name];

    run::run("docker", &args_, &[])
        .chain_err(|| "cargo build failed")?;

    Ok(())
}

#[cfg(unix)]
fn user_id() -> ::libc::uid_t {
    unsafe { ::libc::geteuid() }
}

#[cfg(windows)]
fn user_id() -> u32 {
    panic!("unimplemented user_id");
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        let cd = env::current_dir().expect("unable to get current dir");
        cd.join(path)
    }
}

fn record_test_result(ex_name: &str, c: &ExCrate, toolchain: &str, r: TestResult) -> Result<()> {
    let result_dir = result_dir(ex_name, c, toolchain)?;
    fs::create_dir_all(&result_dir)?;
    let result_file = result_file(ex_name, c, toolchain)?;
    log!("test result! ex: {}, c: {}, tc: {}, r: {}", ex_name, c, toolchain, r);
    log!("file: {}", result_file.display());
    file::write_string(&result_file, &r.to_string())?;
    Ok(())
}

pub fn get_test_result(ex_name: &str, c: &ExCrate, toolchain: &str) -> Result<Option<TestResult>> {
    let result_file = result_file(ex_name, c, toolchain)?;
    if result_file.exists() {
        let s = file::read_string(&result_file)?;
        let r = TestResult::from_str(&s)
            .chain_err(|| format!("invalid test result value: '{}'", s))?;
        Ok(Some(r))
    } else {
        Ok(None)
    }
}

fn find_unstable_features(_ex_name: &str, path: &Path, _rustup_tc: &str) -> Result<TestResult> {
    use walkdir::*;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry.file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
    }

    let mut features = HashSet::new();

    for entry in WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry.chain_err(|| "walk dir")?;
        if !entry.file_name().to_str().map(|s| s.contains(".rs")).unwrap_or(false) { continue }
        if !entry.file_type().is_file() { continue }

        let new_features = parse_features(entry.path())?;

        for feature in new_features {
            features.insert(feature);
        }
    }

    let mut features: Vec<_> = features.into_iter().collect();
    features.sort();
    for feature in features {
        log!("unstable-feature: {}", feature);
    }

    Ok(TestResult::TestPass)
}

fn parse_features(path: &Path) -> Result<Vec<String>> {
    let mut features = Vec::new();
    let contents = file::read_string(path)?;
    for (hash_idx, _) in contents.match_indices('#') {
        fn ten_bytes(s: Option<&str>) -> String {
            if let Some(s) = s {
                if s.len() < 10 {
                    s.to_string()
                } else {
                    s[..10].to_string()
                }
            } else {
                String::from("<none>")
            }
        }
        let contents = &contents[hash_idx + 1..];
        let contents = eat_token(Some(contents), "!").or(Some(contents));
        let contents = eat_token(contents, "[");
        let contents = eat_token(contents, "feature");
        let new_features = parse_list(contents, "(", ")");
        features.extend_from_slice(&new_features);
    }

    fn eat_token<'a>(s: Option<&'a str>, tok: &str) -> Option<&'a str> {
        eat_whitespace(s).and_then(|s| {
            if s.starts_with(tok) {
                Some(&s[tok.len()..])
            } else {
                None
            }
        })
    }

    fn eat_whitespace(s: Option<&str>) -> Option<&str> {
        s.and_then(|s| {
            if let Some(i) = s.find(|c: char| !c.is_whitespace()) {
                Some(&s[i..])
            } else {
                None
            }
        })
    }

    fn parse_list(s: Option<&str>, open: &str, close: &str) -> Vec<String> {
        let s = eat_whitespace(s);
        let s = eat_token(s, open);
        if let Some(s) = s {
            if let Some(i) = s.find(close) {
                let s = &s[..i];
                return s.split(',').map(|s| s.trim().to_string()).collect();
            }
        }

        Vec::new()
    }

    Ok(features)
}
