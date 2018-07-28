use config::Config;
use crates::Crate;
use errors::*;
use ex::*;
use file;
use ref_slice::ref_slice;
use results::{DeleteResults, FileDB, TestResult, WriteResults};
use std::collections::HashSet;
use std::path::Path;
use toolchain::{CargoState, Toolchain};

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

pub struct RunTestResult {
    pub result: TestResult,
    pub skipped: bool,
}

#[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
pub fn run_test<DB: WriteResults>(
    config: &Config,
    action: &str,
    ex: &Experiment,
    tc: &Toolchain,
    krate: &Crate,
    db: &DB,
    quiet: bool,
    test_fn: fn(&Experiment, &Path, &Toolchain, bool) -> Result<TestResult>,
) -> Result<RunTestResult> {
    if let Some(res) = db.get_result(ex, tc, krate)? {
        info!("skipping crate {}. existing result: {}", krate, res);
        Ok(RunTestResult {
            result: res,
            skipped: true,
        })
    } else {
        with_work_crate(ex, tc, krate, |source_path| {
            with_captured_lockfile(config, ex, krate, source_path)?;

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
        false,
    )?;
    toolchain.run_cargo(
        ex,
        source_path,
        &["test", "--frozen", "--no-run"],
        CargoState::Locked,
        quiet,
        false,
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
        false,
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
        false,
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
