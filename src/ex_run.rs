use docker;
use errors::*;
use ex::*;
use file;
use gh_mirrors;
use log;
use model::ExMode;
use std::collections::HashSet;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;
use toolchain::Toolchain;
use util;

pub fn result_dir(ex_name: &str, c: &ExCrate, toolchain: &Toolchain) -> Result<PathBuf> {
    let tc = toolchain.rustup_name();
    Ok(ex_dir(ex_name).join("res").join(tc).join(crate_to_dir(c)?))
}

pub fn result_file(ex_name: &str, c: &ExCrate, toolchain: &Toolchain) -> Result<PathBuf> {
    Ok(result_dir(ex_name, c, toolchain)?.join("results.txt"))
}

pub fn result_log(ex_name: &str, c: &ExCrate, toolchain: &Toolchain) -> Result<PathBuf> {
    Ok(result_dir(ex_name, c, toolchain)?.join("log.txt"))
}

pub fn delete_all_results(ex_name: &str) -> Result<()> {
    let dir = ex_dir(ex_name).join("res");
    if dir.exists() {
        util::remove_dir_all(&dir)?;
    }

    Ok(())
}

fn crate_to_dir(c: &ExCrate) -> Result<String> {
    match *c {
        ExCrate::Version {
            ref name,
            ref version,
        } => Ok(format!("reg/{}-{}", name, version)),
        ExCrate::Repo { ref url, ref sha } => {
            let (org, name) = gh_mirrors::gh_url_to_org_and_name(url)?;
            Ok(format!("gh/{}.{}.{}", org, name, sha))
        }
    }
}

pub fn run_ex_all_tcs(ex_name: &str) -> Result<()> {
    let config = &load_config(ex_name)?;
    run_exts(config, &config.toolchains)
}

pub fn run_ex(ex_name: &str, tc: Toolchain) -> Result<()> {
    let config = load_config(ex_name)?;
    run_exts(&config, &[tc])
}

fn run_exts(config: &Experiment, tcs: &[Toolchain]) -> Result<()> {
    verify_toolchains(config, tcs)?;

    let ex_name = &config.name;
    let crates = ex_crates_and_dirs(ex_name)?;

    // Just for reporting progress
    let total_crates = crates.len() * tcs.len();
    let mut skipped_crates = 0;
    let mut completed_crates = 0;

    // These should add up to total_crates
    let mut sum_errors = 0;
    let mut sum_build_fail = 0;
    let mut sum_test_fail = 0;
    let mut sum_test_pass = 0;

    let start_time = Instant::now();

    let test_fn = match config.mode {
        ExMode::BuildAndTest => test_build_and_test,
        ExMode::BuildOnly => test_build_only,
        ExMode::CheckOnly => test_check_only,
        ExMode::UnstableFeatures => test_find_unstable_features,
    };

    log!("running {} tests", total_crates);
    for (ref c, ref dir) in crates {
        for tc in tcs {
            let r = {
                let existing_result = get_test_result(ex_name, c, tc)?;
                if let Some(r) = existing_result {
                    skipped_crates += 1;

                    log!("skipping crate {}. existing result: {}", c, r);
                    log!("delete result file to rerun test: {}",
                         result_file(ex_name, c, tc)?.display());
                    Ok(r)
                } else {
                    completed_crates += 1;

                    with_work_crate(ex_name, tc, c, |path| {
                        with_frobbed_toml(ex_name, c, path)?;
                        with_captured_lockfile(ex_name, c, path)?;

                        run_single_test(ex_name, c, path, tc, &test_fn)
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
                    record_test_result(ex_name, c, tc, *r);
                }
            }

            match r {
                Err(_) => {
                    sum_errors += 1;
                }
                Ok(TestResult::BuildFail) => sum_build_fail += 1,
                Ok(TestResult::TestFail) => sum_test_fail += 1,
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

            log!("progress: {} / {}",
                 completed_crates + skipped_crates,
                 total_crates);
            log!("{} crates tested in {} s. {:.2} s/crate. {} crates remaining. ~{}",
                 completed_crates,
                 elapsed,
                 seconds_per_test,
                 remaining_tests,
                 remaining_time_str);
            log!("results: {} build-fail / {} test-fail / {} test-pass / {} errors",
                 sum_build_fail,
                 sum_test_fail,
                 sum_test_pass,
                 sum_errors);
        }
    }

    Ok(())
}

fn verify_toolchains(config: &Experiment, tcs: &[Toolchain]) -> Result<()> {
    for tc in tcs {
        if !config.toolchains.contains(tc) {
            bail!("toolchain {} not in experiment", tc.to_string());
        }
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

impl FromStr for TestResult {
    type Err = Error;

    fn from_str(s: &str) -> Result<TestResult> {
        match s {
            "build-fail" => Ok(TestResult::BuildFail),
            "test-fail" => Ok(TestResult::TestFail),
            "test-pass" => Ok(TestResult::TestPass),
            _ => Err(format!("bogus test result: {}", s).into()),
        }
    }
}

impl TestResult {
    fn to_string(&self) -> String {
        match *self {
                TestResult::BuildFail => "build-fail",
                TestResult::TestFail => "test-fail",
                TestResult::TestPass => "test-pass",
            }
            .to_string()
    }
}

fn run_single_test<F>(ex_name: &str,
                      c: &ExCrate,
                      source_path: &Path,
                      toolchain: &Toolchain,
                      f: &F)
                      -> Result<TestResult>
    where F: Fn(&Path, &Path, &str) -> Result<TestResult>
{
    let result_dir = result_dir(ex_name, c, toolchain)?;
    if result_dir.exists() {
        util::remove_dir_all(&result_dir)?;
    }
    fs::create_dir_all(&result_dir)?;
    let log_file = result_log(ex_name, c, toolchain)?;

    log::redirect(&log_file, || {
        log!("testing {} against {} for {}",
             c,
             toolchain.to_string(),
             ex_name);
        let tc = toolchain.rustup_name();
        let target_path = toolchain.target_dir(ex_name);
        f(source_path, &target_path, &tc)
    })
}

fn test_build_and_test(source_path: &Path,
                       target_path: &Path,
                       rustup_tc: &str)
                       -> Result<TestResult> {
    let tc_arg = &format!("+{}", rustup_tc);
    let build_r = docker::run(source_path,
                              target_path,
                              &["cargo", tc_arg, "build", "--frozen"]);
    let mut test_r;

    if build_r.is_ok() {
        // First build, with --no-run
        test_r = Some(docker::run(source_path,
                                  target_path,
                                  &["cargo", tc_arg, "test", "--frozen", "--no-run"]));
        // Then run
        test_r = test_r.map(|_| {
                                docker::run(source_path,
                                            target_path,
                                            &["cargo", tc_arg, "test", "--frozen"])
                            });
    } else {
        test_r = None;
    }

    Ok(match (build_r, test_r) {
           (Err(_), None) => TestResult::BuildFail,
           (Ok(_), Some(Err(_))) => TestResult::TestFail,
           (Ok(_), Some(Ok(_))) => TestResult::TestPass,
           (_, _) => unreachable!(),
       })
}

fn test_build_only(source_path: &Path, target_path: &Path, rustup_tc: &str) -> Result<TestResult> {
    let tc_arg = &format!("+{}", rustup_tc);
    let r = docker::run(source_path,
                        target_path,
                        &["cargo", tc_arg, "build", "--frozen"]);

    if r.is_ok() {
        Ok(TestResult::TestPass)
    } else {
        Ok(TestResult::BuildFail)
    }
}

fn test_check_only(source_path: &Path, target_path: &Path, rustup_tc: &str) -> Result<TestResult> {
    let tc_arg = &format!("+{}", rustup_tc);
    let r = docker::run(source_path,
                        target_path,
                        &["cargo", tc_arg, "check", "--frozen"]);

    if r.is_ok() {
        Ok(TestResult::TestPass)
    } else {
        Ok(TestResult::BuildFail)
    }
}

fn record_test_result(ex_name: &str,
                      c: &ExCrate,
                      toolchain: &Toolchain,
                      r: TestResult)
                      -> Result<()> {
    let result_dir = result_dir(ex_name, c, toolchain)?;
    fs::create_dir_all(&result_dir)?;
    let result_file = result_file(ex_name, c, toolchain)?;
    log!("test result! ex: {}, c: {}, tc: {}, r: {}",
         ex_name,
         c,
         toolchain.to_string(),
         r);
    log!("file: {}", result_file.display());
    file::write_string(&result_file, &r.to_string())?;
    Ok(())
}

pub fn get_test_result(ex_name: &str,
                       c: &ExCrate,
                       toolchain: &Toolchain)
                       -> Result<Option<TestResult>> {
    let result_file = result_file(ex_name, c, toolchain)?;
    if result_file.exists() {
        let s = file::read_string(&result_file)?;
        let r = s.parse::<TestResult>()
            .chain_err(|| format!("invalid test result value: '{}'", s))?;
        Ok(Some(r))
    } else {
        Ok(None)
    }
}

fn test_find_unstable_features(source_path: &Path,
                               target_path: &Path,
                               _rustup_tc: &str)
                               -> Result<TestResult> {
    use walkdir::*;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }

    let mut features = HashSet::new();

    for entry in WalkDir::new(source_path)
            .into_iter()
            .filter_entry(|e| !is_hidden(e)) {
        let entry = entry.chain_err(|| "walk dir")?;
        if !entry
                .file_name()
                .to_str()
                .map(|s| s.contains(".rs"))
                .unwrap_or(false) {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }

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
        let contents = &contents[hash_idx + 1..];
        let contents = eat_token(Some(contents), "!").or_else(|| Some(contents));
        let contents = eat_token(contents, "[");
        let contents = eat_token(contents, "feature");
        let new_features = parse_list(contents, "(", ")");
        features.extend_from_slice(&new_features);
    }

    fn eat_token<'a>(s: Option<&'a str>, tok: &str) -> Option<&'a str> {
        eat_whitespace(s).and_then(|s| if s.starts_with(tok) {
                                       Some(&s[tok.len()..])
                                   } else {
                                       None
                                   })
    }

    fn eat_whitespace(s: Option<&str>) -> Option<&str> {
        s.and_then(|s| if let Some(i) = s.find(|c: char| !c.is_whitespace()) {
                       Some(&s[i..])
                   } else {
                       None
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
