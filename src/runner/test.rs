use crate::dirs;
use crate::docker::{DockerError, MountPerms};
use crate::prelude::*;
use crate::results::{EncodingType, FailureReason, TestResult, WriteResults};
use crate::run::{RunCommand, RunCommandError};
use crate::runner::tasks::TaskCtx;
use crate::tools::CARGO;
use failure::Error;
use std::path::Path;

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

    let rustflags_env = if let Some(&"doc") = args.get(0) {
        "RUSTDOCFLAGS"
    } else {
        "RUSTFLAGS"
    };

    RunCommand::new(CARGO.toolchain(ctx.toolchain))
        .args(args)
        .quiet(ctx.quiet)
        .cd(source_path)
        .env(
            "CARGO_TARGET_DIR",
            dirs::container::TARGET_DIR.to_str().unwrap(),
        )
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "full")
        .env(rustflags_env, rustflags)
        .sandboxed(&ctx.docker_env)
        .mount(
            target_dir,
            &*dirs::container::TARGET_DIR,
            MountPerms::ReadWrite,
        )
        .memory_limit(Some(ctx.config.sandbox.memory_limit))
        .run()?;

    Ok(())
}

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
    } else {
        let source_path = crate::dirs::crate_source_dir(ctx.experiment, ctx.toolchain, ctx.krate);
        let log_storage = ctx
            .state
            .lock()
            .prepare_logs
            .get(&ctx.krate)
            .map(|s| s.duplicate());
        ctx.db.record_result(
            ctx.experiment,
            ctx.toolchain,
            ctx.krate,
            log_storage,
            ctx.config,
            EncodingType::Plain,
            || {
                info!(
                    "{} {} against {} for {}",
                    action,
                    ctx.krate,
                    ctx.toolchain.to_string(),
                    ctx.experiment.name
                );
                test_fn(ctx, &source_path)
            },
        )?;
    }
    Ok(())
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

pub(super) fn test_clippy_only<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    source_path: &Path,
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        ctx,
        source_path,
        &["clippy", "--frozen", "--all", "--all-targets"],
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub(super) fn test_rustdoc<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    source_path: &Path,
) -> Fallible<TestResult> {
    let res = run_cargo(
        ctx,
        source_path,
        &["doc", "--frozen", "--no-deps", "--document-private-items"],
    );

    // Make sure to remove the built documentation
    // There is no point in storing it after the build is done
    let target_dir = ctx.toolchain.target_dir(&ctx.experiment.name);
    crate::utils::fs::remove_dir_all(&target_dir.join("doc"))?;

    if let Err(err) = res {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}
