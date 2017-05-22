use docker;
use errors::*;
use ex::*;
use file;
use model::ExMode;
use ref_slice::ref_slice;
use results::{CrateResultWriter, ExperimentResultDB, FileDB, TestResult};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;
use toolchain::Toolchain;
use util;


pub fn delete_all_results(ex_name: &str) -> Result<()> {
    let ex = &load_config(ex_name)?;
    let db = FileDB::for_experiment(ex);
    db.delete_all_results()
}

pub fn delete_result(ex_name: &str, tc: Option<&Toolchain>, crate_: &ExCrate) -> Result<()> {
    let ex = &load_config(ex_name)?;
    let db = FileDB::for_experiment(ex);

    let tcs = tc.map(ref_slice).unwrap_or(&ex.toolchains);
    for tc in tcs {
        let writer = db.for_crate(crate_, tc);
        writer.delete_result()?;
    }

    Ok(())
}

pub fn run_ex_all_tcs(ex_name: &str) -> Result<()> {
    let config = &load_config(ex_name)?;
    run_exts(config, &config.toolchains)
}

pub fn run_ex(ex_name: &str, tc: Toolchain) -> Result<()> {
    let config = load_config(ex_name)?;
    run_exts(&config, &[tc])
}

fn run_exts(ex: &Experiment, tcs: &[Toolchain]) -> Result<()> {
    let db = FileDB::for_experiment(ex);
    verify_toolchains(ex, tcs)?;

    let crates = ex_crates_and_dirs(ex)?;

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

    let test_fn = match ex.mode {
        ExMode::BuildAndTest => test_build_and_test,
        ExMode::BuildOnly => test_build_only,
        ExMode::CheckOnly => test_check_only,
        ExMode::UnstableFeatures => test_find_unstable_features,
    };

    info!("running {} tests", total_crates);
    for (ref c, _) in crates {
        for tc in tcs {
            let writer = db.for_crate(c, tc);
            let r = {
                let existing_result = writer.get_test_results()?;
                if let Some(r) = existing_result {
                    skipped_crates += 1;

                    info!("skipping crate {}. existing result: {}", c, r);
                    info!("delete result file to rerun test: \
                           \"cargobomb delete-result {} --toolchain {} {}\"",
                          ex.name,
                          tc.to_string(),
                          c);
                    Ok(r)
                } else {
                    completed_crates += 1;

                    with_work_crate(ex, tc, c, |source_path| {
                        with_frobbed_toml(ex, c, source_path)?;
                        with_captured_lockfile(ex, c, source_path)?;

                        writer.record_results(|| {
                            info!("testing {} against {} for {}", c, tc.to_string(), ex.name);
                            let target_path = tc.target_dir(&ex.name);
                            test_fn(source_path, &target_path, &tc.rustup_name())
                        })
                    })
                }
            };

            match r {
                Err(ref e) => {
                    error!("error testing crate {}:  {}", c, e);
                    util::report_error(e);
                }
                Ok(ref r) => {
                    // FIXME: Should errors be recorded?
                    info!("test result! ex: {}, c: {}, tc: {}, r: {}",
                          ex.name,
                          c,
                          tc.to_string(),
                          r);
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

            info!("progress: {} / {}",
                  completed_crates + skipped_crates,
                  total_crates);
            info!("{} crates tested in {} s. {:.2} s/crate. {} crates remaining. ~{}",
                  completed_crates,
                  elapsed,
                  seconds_per_test,
                  remaining_tests,
                  remaining_time_str);
            info!("results: {} build-fail / {} test-fail / {} test-pass / {} errors",
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

fn test_find_unstable_features(source_path: &Path,
                               _target_path: &Path,
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
        info!("unstable-feature: {}", feature);
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
