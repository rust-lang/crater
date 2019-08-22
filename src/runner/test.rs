use crate::prelude::*;
use crate::results::{BrokenReason, EncodingType, FailureReason, TestResult, WriteResults};
use crate::runner::tasks::TaskCtx;
use crate::runner::OverrideResult;
use failure::Error;
use rustwide::cmd::{CommandError, SandboxBuilder};
use rustwide::{Build, PrepareError};

fn failure_reason(err: &Error) -> FailureReason {
    for cause in err.iter_chain() {
        if let Some(&CommandError::SandboxOOM) = cause.downcast_ctx() {
            return FailureReason::OOM;
        } else if let Some(&CommandError::NoOutputFor(_)) = cause.downcast_ctx() {
            return FailureReason::Timeout;
        } else if let Some(&CommandError::Timeout(_)) = cause.downcast_ctx() {
            return FailureReason::Timeout;
        }
    }

    FailureReason::Unknown
}

fn detect_broken<T>(res: Result<T, Error>) -> Result<T, Error> {
    match res {
        Ok(ok) => Ok(ok),
        Err(err) => {
            let mut reason = None;
            for cause in err.iter_chain() {
                if let Some(&PrepareError::MissingCargoToml) = cause.downcast_ctx() {
                    reason = Some(BrokenReason::CargoToml);
                } else if let Some(&PrepareError::InvalidCargoTomlSyntax) = cause.downcast_ctx() {
                    reason = Some(BrokenReason::CargoToml);
                } else if let Some(&PrepareError::YankedDependencies) = cause.downcast_ctx() {
                    reason = Some(BrokenReason::Yanked);
                } else {
                    continue;
                }
                break;
            }
            if let Some(reason) = reason {
                Err(err
                    .context(OverrideResult(TestResult::BrokenCrate(reason)))
                    .into())
            } else {
                Err(err)
            }
        }
    }
}

fn run_cargo<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
    args: &[&str],
) -> Fallible<()> {
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

    let mut command = build_env
        .cargo()
        .args(args)
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "full")
        .env(rustflags_env, rustflags);
    if ctx.quiet {
        command = command.no_output_timeout(None);
    }
    command.run()?;

    Ok(())
}

pub(super) fn run_test<DB: WriteResults>(
    action: &str,
    ctx: &TaskCtx<DB>,
    test_fn: fn(&TaskCtx<DB>, &Build) -> Fallible<TestResult>,
) -> Fallible<()> {
    if let Some(res) = ctx
        .db
        .get_result(ctx.experiment, ctx.toolchain, ctx.krate)?
    {
        info!("skipping crate {}. existing result: {}", ctx.krate, res);
    } else {
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
                let sandbox = SandboxBuilder::new()
                    .memory_limit(Some(ctx.config.sandbox.memory_limit.to_bytes()))
                    .enable_networking(false);
                detect_broken(ctx.build_dir.lock().unwrap().build(
                    &ctx.toolchain,
                    &ctx.krate.to_rustwide(),
                    sandbox,
                    |build| test_fn(ctx, build),
                ))
            },
        )?;
    }
    Ok(())
}

fn build<DB: WriteResults>(ctx: &TaskCtx<DB>, build_env: &Build) -> Fallible<()> {
    run_cargo(ctx, build_env, &["build", "--frozen"])?;
    run_cargo(ctx, build_env, &["test", "--frozen", "--no-run"])?;
    Ok(())
}

fn test<DB: WriteResults>(ctx: &TaskCtx<DB>, build_env: &Build) -> Fallible<()> {
    run_cargo(ctx, build_env, &["test", "--frozen"])
}

pub(super) fn test_build_and_test<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
) -> Fallible<TestResult> {
    let build_r = build(ctx, build_env);
    let test_r = if build_r.is_ok() {
        Some(test(ctx, build_env))
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
    build_env: &Build,
) -> Fallible<TestResult> {
    if let Err(err) = build(ctx, build_env) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestSkipped)
    }
}

pub(super) fn test_check_only<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        ctx,
        build_env,
        &["check", "--frozen", "--all", "--all-targets"],
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub(super) fn test_clippy_only<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        ctx,
        build_env,
        &["clippy", "--frozen", "--all", "--all-targets"],
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub(super) fn test_rustdoc<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
) -> Fallible<TestResult> {
    let res = run_cargo(
        ctx,
        build_env,
        &["doc", "--frozen", "--no-deps", "--document-private-items"],
    );

    // Make sure to remove the built documentation
    // There is no point in storing it after the build is done
    std::fs::remove_dir_all(&build_env.host_target_dir().join("doc"))?;

    if let Err(err) = res {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}
