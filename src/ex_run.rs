use config::Config;
use crates::Crate;
use errors::*;
use ex::*;
use file;
use ref_slice::ref_slice;
use results::{DeleteResults, FileDB, TestResult, WriteResults};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;
use toolchain::{CargoState, Toolchain};
use util;

pub fn delete_all_results(ex_name: &str) -> Result<()> {
    let ex = &Experiment::load(ex_name)?;
    let db = FileDB::default();
    db.delete_all_results(ex)
}

pub fn delete_result(ex_name: &str, tc: Option<&Toolchain>, krate: &Crate) -> Result<()> {
    let ex = &Experiment::load(ex_name)?;
    let db = FileDB::default();

    let tcs = tc.map(ref_slice).unwrap_or(&ex.toolchains);
    for tc in tcs {
        db.delete_result(ex, tc, krate)?;
    }

    Ok(())
}

pub fn run_ex_all_tcs(ex_name: &str, config: &Config) -> Result<()> {
    let ex = &Experiment::load(ex_name)?;
    run_exts(ex, &ex.toolchains, config)
}

pub fn run_ex(ex_name: &str, tc: Toolchain, config: &Config) -> Result<()> {
    let ex = Experiment::load(ex_name)?;
    run_exts(&ex, &[tc], config)
}

fn run_exts(ex: &Experiment, tcs: &[Toolchain], config: &Config) -> Result<()> {
    let db = FileDB::default();
    verify_toolchains(ex, tcs)?;

    // Just for reporting progress
    let total_crates = ex.crates.len() * tcs.len();
    let mut skipped_crates = 0;
    let mut completed_crates = 0;

    // These should add up to total_crates
    let mut sum_errors = 0;
    let mut sum_build_fail = 0;
    let mut sum_test_fail = 0;
    let mut sum_test_skipped = 0;
    let mut sum_test_pass = 0;

    let start_time = Instant::now();

    info!("running {} tests", total_crates);
    for c in &ex.crates {
        if config.should_skip(c) {
            info!("skipping crate {}: blacklisted in config.toml", c);

            skipped_crates += tcs.len();
            continue;
        }

        let test_fn = match ex.mode {
            // Don't execute tests if the crate is blacklisted
            ExMode::BuildAndTest if config.should_skip_tests(c) => test_build_only,
            ExMode::BuildAndTest => test_build_and_test,

            ExMode::BuildOnly => test_build_only,
            ExMode::CheckOnly => test_check_only,
            ExMode::UnstableFeatures => test_find_unstable_features,
        };

        for tc in tcs {
            let r = run_test("testing", ex, tc, c, &db, config.is_quiet(c), test_fn);

            match r {
                Err(ref e) => {
                    error!("error testing crate {}:  {}", c, e);
                    util::report_error(e);
                }
                Ok(RunTestResult {
                    ref result,
                    skipped,
                }) => {
                    // FIXME: Should errors be recorded?
                    info!(
                        "test result! ex: {}, c: {}, tc: {}, r: {}",
                        ex.name,
                        c,
                        tc.to_string(),
                        result,
                    );

                    if skipped {
                        skipped_crates += 1;
                    } else {
                        completed_crates += 1;
                    }
                }
            }

            match r {
                Err(_) => {
                    sum_errors += 1;
                }
                Ok(RunTestResult { skipped: true, .. }) => {}
                Ok(RunTestResult {
                    result: TestResult::BuildFail,
                    ..
                }) => sum_build_fail += 1,
                Ok(RunTestResult {
                    result: TestResult::TestFail,
                    ..
                }) => sum_test_fail += 1,
                Ok(RunTestResult {
                    result: TestResult::TestSkipped,
                    ..
                }) => sum_test_skipped += 1,
                Ok(RunTestResult {
                    result: TestResult::TestPass,
                    ..
                }) => sum_test_pass += 1,
                Ok(RunTestResult {
                    result: TestResult::Error,
                    ..
                }) => unreachable!("error results are not supported with legacy run"),
            }

            let elapsed = Instant::now().duration_since(start_time).as_secs();
            let seconds_per_test = if completed_crates > 0 {
                (elapsed as f64) / (completed_crates as f64)
            } else {
                0.0
            };
            let remaining_tests = total_crates - completed_crates - skipped_crates;
            let remaining_time = remaining_tests * seconds_per_test as usize;

            let remaining_time_str = if remaining_time < 60 {
                // 1 minute
                format!("{:0} seconds", remaining_time)
            } else if remaining_time < 60 * 60 {
                // 1 hour
                format!("{:0} minutes", remaining_time / 60)
            } else {
                format!("{:0} hours", remaining_time / 60 / 60)
            };

            info!(
                "progress: {} / {}",
                completed_crates + skipped_crates,
                total_crates
            );
            info!(
                "{} crates tested in {} s. {:.2} s/crate. {} crates remaining. ~{}",
                completed_crates, elapsed, seconds_per_test, remaining_tests, remaining_time_str
            );
            info!(
                "results: {} build-fail / {} test-fail / {} test-skipped / {} test-pass / {} errors",
                sum_build_fail, sum_test_fail, sum_test_skipped, sum_test_pass, sum_errors
            );
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

pub struct RunTestResult {
    pub result: TestResult,
    pub skipped: bool,
}

pub fn run_test<DB: WriteResults>(
    action: &str,
    ex: &Experiment,
    tc: &Toolchain,
    krate: &Crate,
    db: &DB,
    quiet: bool,
    test_fn: fn(&Experiment, &Path, &Toolchain, bool) -> Result<TestResult>,
) -> Result<RunTestResult> {
    if let Some(res) = db.already_executed(ex, tc, krate)? {
        info!("skipping crate {}. existing result: {}", krate, res);
        Ok(RunTestResult {
            result: res,
            skipped: true,
        })
    } else {
        with_work_crate(ex, tc, krate, |source_path| {
            with_frobbed_toml(ex, krate, source_path)?;
            with_captured_lockfile(ex, krate, source_path)?;

            db.record_result(ex, tc, krate, || {
                info!(
                    "{} {} against {} for {}",
                    action,
                    krate,
                    tc.to_string(),
                    ex.name
                );
                test_fn(ex, source_path, tc, quiet)
            })
        }).map(|result| RunTestResult {
            result,
            skipped: false,
        })
    }
}

fn build(ex: &Experiment, source_path: &Path, toolchain: &Toolchain, quiet: bool) -> Result<()> {
    toolchain.run_cargo(
        ex,
        source_path,
        &["build", "--frozen"],
        CargoState::Locked,
        quiet,
    )?;
    toolchain.run_cargo(
        ex,
        source_path,
        &["test", "--frozen", "--no-run"],
        CargoState::Locked,
        quiet,
    )?;
    Ok(())
}

fn test(ex: &Experiment, source_path: &Path, toolchain: &Toolchain, quiet: bool) -> Result<()> {
    toolchain.run_cargo(
        ex,
        source_path,
        &["test", "--frozen"],
        CargoState::Locked,
        quiet,
    )
}

pub fn test_build_and_test(
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<TestResult> {
    let build_r = build(ex, source_path, toolchain, quiet);
    let test_r = if build_r.is_ok() {
        Some(test(ex, source_path, toolchain, quiet))
    } else {
        None
    };

    Ok(match (build_r, test_r) {
        (Err(_), None) => TestResult::BuildFail,
        (Ok(_), Some(Err(_))) => TestResult::TestFail,
        (Ok(_), Some(Ok(_))) => TestResult::TestPass,
        (_, _) => unreachable!(),
    })
}

pub fn test_build_only(
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<TestResult> {
    let r = build(ex, source_path, toolchain, quiet);
    if r.is_ok() {
        Ok(TestResult::TestSkipped)
    } else {
        Ok(TestResult::BuildFail)
    }
}

pub fn test_check_only(
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<TestResult> {
    let r = toolchain.run_cargo(
        ex,
        source_path,
        &["check", "--frozen", "--all", "--all-targets"],
        CargoState::Locked,
        quiet,
    );

    if r.is_ok() {
        Ok(TestResult::TestPass)
    } else {
        Ok(TestResult::BuildFail)
    }
}

pub fn test_find_unstable_features(
    _ex: &Experiment,
    source_path: &Path,
    _toolchain: &Toolchain,
    _quiet: bool,
) -> Result<TestResult> {
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
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry.chain_err(|| "walk dir")?;
        if !entry
            .file_name()
            .to_str()
            .map(|s| s.contains(".rs"))
            .unwrap_or(false)
        {
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
