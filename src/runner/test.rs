use config::Config;
use crates::Crate;
use docker::MountPerms;
use errors::*;
use experiments::Experiment;
use results::{TestResult, WriteResults};
use run::RunCommand;
use runner::prepare::{with_captured_lockfile, with_frobbed_toml, with_work_crate};
use std::path::Path;
use toolchain::Toolchain;
use tools::CARGO;

fn run_cargo(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
    args: &[&str],
) -> Result<()> {
    let target_dir = toolchain.target_dir(&ex.name);
    ::std::fs::create_dir_all(&target_dir)?;

    let mut rustflags = format!("--cap-lints={}", ex.cap_lints.to_str());
    if let Some(ref tc_rustflags) = toolchain.rustflags {
        rustflags.push(' ');
        rustflags.push_str(tc_rustflags);
    }

    RunCommand::new(CARGO.toolchain(toolchain))
        .args(args)
        .quiet(quiet)
        .cd(source_path)
        .env("CARGO_TARGET_DIR", "/target")
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "full")
        .env("RUSTFLAGS", rustflags)
        .sandboxed()
        .mount(target_dir, "/target", MountPerms::ReadWrite)
        .memory_limit(Some(config.sandbox.memory_limit))
        .run()
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
    test_fn: fn(&Config, &Experiment, &Path, &Toolchain, bool) -> Result<TestResult>,
) -> Result<RunTestResult> {
    if let Some(res) = db.get_result(ex, tc, krate)? {
        info!("skipping crate {}. existing result: {}", krate, res);
        Ok(RunTestResult {
            result: res,
            skipped: true,
        })
    } else {
        with_work_crate(ex, tc, krate, |source_path| {
            with_frobbed_toml(ex, krate, source_path)?;
            with_captured_lockfile(config, ex, krate, source_path)?;

            db.record_result(ex, tc, krate, || {
                info!(
                    "{} {} against {} for {}",
                    action,
                    krate,
                    tc.to_string(),
                    ex.name
                );
                test_fn(config, ex, source_path, tc, quiet)
            })
        }).map(|result| RunTestResult {
            result,
            skipped: false,
        })
    }
}

fn build(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<()> {
    run_cargo(
        config,
        ex,
        source_path,
        toolchain,
        quiet,
        &["build", "--frozen"],
    )?;
    run_cargo(
        config,
        ex,
        source_path,
        toolchain,
        quiet,
        &["test", "--frozen", "--no-run"],
    )?;
    Ok(())
}

fn test(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<()> {
    run_cargo(
        config,
        ex,
        source_path,
        toolchain,
        quiet,
        &["test", "--frozen"],
    )
}

pub fn test_build_and_test(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<TestResult> {
    let build_r = build(config, ex, source_path, toolchain, quiet);
    let test_r = if build_r.is_ok() {
        Some(test(config, ex, source_path, toolchain, quiet))
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
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<TestResult> {
    let r = build(config, ex, source_path, toolchain, quiet);
    if r.is_ok() {
        Ok(TestResult::TestSkipped)
    } else {
        Ok(TestResult::BuildFail)
    }
}

pub fn test_check_only(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Result<TestResult> {
    if run_cargo(
        config,
        ex,
        source_path,
        toolchain,
        quiet,
        &["check", "--frozen", "--all", "--all-targets"],
    ).is_ok()
    {
        Ok(TestResult::TestPass)
    } else {
        Ok(TestResult::BuildFail)
    }
}
