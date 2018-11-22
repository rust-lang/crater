use config::Config;
use crates::Crate;
use docker::DockerError;
use docker::MountPerms;
use experiments::Experiment;
use failure::Error;
use prelude::*;
use results::EncodingType;
use results::{FailureReason, TestResult, WriteResults};
use run::{RunCommand, RunCommandError};
use runner::prepare::{with_captured_lockfile, with_frobbed_toml, with_work_crate};
use std::path::Path;
use toolchain::Toolchain;
use tools::CARGO;

fn failure_reason(err: &Error) -> FailureReason {
    for cause in err.iter_chain() {
        if let Some(&DockerError::ContainerOOM) = cause.downcast_ctx() {
            return FailureReason::OOM;
        } else if let Some(&RunCommandError::NoOutputFor(_)) = cause.downcast_ctx() {
            return FailureReason::Timeout;
        } else if let Some(&RunCommandError::Timeout(_)) = cause.downcast_ctx() {
            return FailureReason::Timeout;
        }
    }

    FailureReason::Unknown
}

fn run_cargo(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
    args: &[&str],
) -> Fallible<()> {
    let target_dir = toolchain.target_dir(&ex.name);
    ::std::fs::create_dir_all(&target_dir)?;

    let mut rustflags = format!("--cap-lints={}", ex.cap_lints.to_str());
    if let Some(ref tc_rustflags) = toolchain.rustflags {
        rustflags.push(' ');
        rustflags.push_str(tc_rustflags);
    }

    let rustflags_env = if let Some(&"doc") = args.get(0) {
        "RUSTDOCFLAGS"
    } else {
        "RUSTFLAGS"
    };

    RunCommand::new(CARGO.toolchain(toolchain))
        .args(args)
        .quiet(quiet)
        .cd(source_path)
        .env("CARGO_TARGET_DIR", "/target")
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "full")
        .env(rustflags_env, rustflags)
        .sandboxed()
        .mount(target_dir, "/target", MountPerms::ReadWrite)
        .memory_limit(Some(config.sandbox.memory_limit))
        .run()?;

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
    test_fn: fn(&Config, &Experiment, &Path, &Toolchain, bool) -> Fallible<TestResult>,
) -> Fallible<RunTestResult> {
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

            db.record_result(
                ex,
                tc,
                krate,
                || {
                    info!(
                        "{} {} against {} for {}",
                        action,
                        krate,
                        tc.to_string(),
                        ex.name
                    );
                    test_fn(config, ex, source_path, tc, quiet)
                },
                EncodingType::Plain,
            )
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
) -> Fallible<()> {
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
) -> Fallible<()> {
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
) -> Fallible<TestResult> {
    let build_r = build(config, ex, source_path, toolchain, quiet);
    let test_r = if build_r.is_ok() {
        Some(test(config, ex, source_path, toolchain, quiet))
    } else {
        None
    };

    Ok(match (build_r, test_r) {
        (Err(err), None) => TestResult::BuildFail(failure_reason(&err)),
        (Ok(_), Some(Err(err))) => TestResult::TestFail(failure_reason(&err)),
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
) -> Fallible<TestResult> {
    if let Err(err) = build(config, ex, source_path, toolchain, quiet) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestSkipped)
    }
}

pub fn test_check_only(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        config,
        ex,
        source_path,
        toolchain,
        quiet,
        &["check", "--frozen", "--all", "--all-targets"],
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub fn test_rustdoc(
    config: &Config,
    ex: &Experiment,
    source_path: &Path,
    toolchain: &Toolchain,
    quiet: bool,
) -> Fallible<TestResult> {
    let res = run_cargo(
        config,
        ex,
        source_path,
        toolchain,
        quiet,
        &["doc", "--frozen", "--no-deps", "--document-private-items"],
    );

    // Make sure to remove the built documentation
    // There is no point in storing it after the build is done
    let target_dir = toolchain.target_dir(&ex.name);
    ::utils::fs::remove_dir_all(&target_dir.join("doc"))?;

    if let Err(err) = res {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}
