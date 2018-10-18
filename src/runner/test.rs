use docker::DockerError;
use docker::MountPerms;
use failure::Error;
use prelude::*;
use results::{FailureReason, TestResult, WriteResults};
use run::{RunCommand, RunCommandError};
use runner::prepare::{with_captured_lockfile, with_frobbed_toml, with_work_crate};
use runner::tasks::TaskCtx;
use std::path::Path;
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

fn run_cargo<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    source_path: &Path,
    args: &[&str],
) -> Fallible<()> {
    let target_dir = ctx.toolchain.target_dir(&ctx.experiment.name);
    ::std::fs::create_dir_all(&target_dir)?;

    let mut rustflags = format!("--cap-lints={}", ctx.experiment.cap_lints.to_str());
    if let Some(ref tc_rustflags) = ctx.toolchain.rustflags {
        rustflags.push(' ');
        rustflags.push_str(tc_rustflags);
    }

    RunCommand::new(CARGO.toolchain(ctx.toolchain))
        .args(args)
        .quiet(ctx.quiet)
        .cd(source_path)
        .env("CARGO_TARGET_DIR", "/target")
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "full")
        .env("RUSTFLAGS", rustflags)
        .sandboxed(&ctx.docker_env)
        .mount(target_dir, "/target", MountPerms::ReadWrite)
        .memory_limit(Some(ctx.config.sandbox.memory_limit))
        .run()?;

    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
pub(super) fn run_test<DB: WriteResults>(
    action: &str,
    ctx: &TaskCtx<DB>,
    test_fn: fn(&TaskCtx<DB>, &Path) -> Fallible<TestResult>,
) -> Fallible<()> {
    if let Some(res) = ctx
        .db
        .get_result(ctx.experiment, ctx.toolchain, ctx.krate)?
    {
        info!("skipping crate {}. existing result: {}", ctx.krate, res);
        Ok(())
    } else {
        with_work_crate(ctx.experiment, ctx.toolchain, ctx.krate, |source_path| {
            with_frobbed_toml(ctx.experiment, ctx.krate, source_path)?;
            with_captured_lockfile(ctx.config, ctx.experiment, ctx.krate, source_path)?;

            ctx.db
                .record_result(ctx.experiment, ctx.toolchain, ctx.krate, || {
                    info!(
                        "{} {} against {} for {}",
                        action,
                        ctx.krate,
                        ctx.toolchain.to_string(),
                        ctx.experiment.name
                    );
                    test_fn(ctx, source_path)
                })
        })?;
        Ok(())
    }
}

fn build<DB: WriteResults>(ctx: &TaskCtx<DB>, source_path: &Path) -> Fallible<()> {
    run_cargo(ctx, source_path, &["build", "--frozen"])?;
    run_cargo(ctx, source_path, &["test", "--frozen", "--no-run"])?;
    Ok(())
}

fn test<DB: WriteResults>(ctx: &TaskCtx<DB>, source_path: &Path) -> Fallible<()> {
    run_cargo(ctx, source_path, &["test", "--frozen"])
}

pub(super) fn test_build_and_test<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    source_path: &Path,
) -> Fallible<TestResult> {
    let build_r = build(ctx, source_path);
    let test_r = if build_r.is_ok() {
        Some(test(ctx, source_path))
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

pub(super) fn test_build_only<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    source_path: &Path,
) -> Fallible<TestResult> {
    if let Err(err) = build(ctx, source_path) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestSkipped)
    }
}

pub(super) fn test_check_only<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    source_path: &Path,
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        ctx,
        source_path,
        &["check", "--frozen", "--all", "--all-targets"],
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}
