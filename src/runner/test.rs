use crate::crates::Crate;
use crate::prelude::*;
use crate::results::DiagnosticCode;
use crate::results::{BrokenReason, EncodingType, FailureReason, TestResult, WriteResults};
use crate::runner::tasks::TaskCtx;
use crate::runner::OverrideResult;
use cargo_metadata::diagnostic::DiagnosticLevel;
use cargo_metadata::{Message, Metadata, PackageId};
use failure::Error;
use remove_dir_all::remove_dir_all;
use rustwide::cmd::{CommandError, ProcessLinesActions, SandboxBuilder};
use rustwide::{Build, PrepareError};
use std::collections::{BTreeSet, HashSet};
use std::convert::TryFrom;

fn failure_reason(err: &Error) -> FailureReason {
    for cause in err.iter_chain() {
        if let Some(&CommandError::SandboxOOM) = cause.downcast_ctx() {
            return FailureReason::OOM;
        } else if let Some(&CommandError::NoOutputFor(_) | &CommandError::Timeout(_)) =
            cause.downcast_ctx()
        {
            return FailureReason::Timeout;
        } else if let Some(reason) = cause.downcast_ctx::<FailureReason>() {
            return reason.clone();
        }
    }

    FailureReason::Unknown
}

pub(super) fn detect_broken<T>(res: Result<T, Error>) -> Result<T, Error> {
    match res {
        Ok(ok) => Ok(ok),
        Err(err) => {
            let mut reason = None;
            for cause in err.iter_chain() {
                if let Some(error) = cause.downcast_ctx() {
                    reason = match *error {
                        PrepareError::MissingCargoToml => Some(BrokenReason::CargoToml),
                        PrepareError::InvalidCargoTomlSyntax => Some(BrokenReason::CargoToml),
                        PrepareError::YankedDependencies => Some(BrokenReason::Yanked),
                        PrepareError::MissingDependencies => {
                            Some(BrokenReason::MissingDependencies)
                        }
                        PrepareError::PrivateGitRepository => {
                            Some(BrokenReason::MissingGitRepository)
                        }
                        _ => None,
                    }
                }
                if reason.is_some() {
                    break;
                }
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

fn get_local_packages(build_env: &Build) -> Fallible<HashSet<PackageId>> {
    Ok(build_env
        .cargo()
        .args(&["metadata", "--no-deps", "--format-version=1"])
        .log_output(false)
        .run_capture()?
        .stdout_lines()
        .iter()
        .filter_map(|line| serde_json::from_str::<Metadata>(line).ok())
        .flat_map(|metadata| metadata.packages.into_iter().map(|pkg| pkg.id))
        .collect::<HashSet<_>>())
}

fn run_cargo<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
    args: &[&str],
    check_errors: bool,
    local_packages_id: &HashSet<PackageId>,
) -> Fallible<()> {
    let mut rustflags = format!("--cap-lints={}", ctx.experiment.cap_lints.to_str());
    if let Some(ref tc_rustflags) = ctx.toolchain.rustflags {
        rustflags.push(' ');
        rustflags.push_str(tc_rustflags);
    }
    // Limit debuginfo in an effort to decrease linker peak memory usage, while retaining enough
    // information to get useful backtraces.
    rustflags.push_str(" -Cdebuginfo=1");

    let rustflags_env = if let Some(&"doc") = args.get(0) {
        "RUSTDOCFLAGS"
    } else {
        "RUSTFLAGS"
    };

    let mut did_ice = false;
    let mut error_codes = BTreeSet::new();
    let mut deps = BTreeSet::new();

    let mut detect_error = |line: &str, actions: &mut ProcessLinesActions| {
        // Avoid trying to deserialize non JSON output
        if !line.starts_with('{') {
            return;
        }

        let message = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(_) => return,
        };

        match message {
            Message::CompilerMessage(compiler_message) => {
                let inner_message = compiler_message.message;
                match (inner_message.level, &compiler_message.package_id) {
                    // the only local crate in a well defined job is the crate currently being tested
                    (DiagnosticLevel::Error, pkgid) if local_packages_id.contains(pkgid) => {
                        if let Some(code) = inner_message.code {
                            error_codes.insert(DiagnosticCode::from(code.code));
                        }
                    }
                    (DiagnosticLevel::Ice, pkgid) if local_packages_id.contains(pkgid) => {
                        did_ice = true
                    }
                    // If the error is in a crate that is not local then it's referred to a dependency
                    // of the current crate
                    (DiagnosticLevel::Error, pkgid) => {
                        if let Ok(krate) = Crate::try_from(pkgid) {
                            deps.insert(krate);
                        }
                    }
                    (DiagnosticLevel::Ice, pkgid) => {
                        if let Ok(krate) = Crate::try_from(pkgid) {
                            deps.insert(krate);
                        }
                    }
                    _ => (),
                }

                actions.replace_with_lines(inner_message.rendered.unwrap_or_default().split('\n'));
            }
            _ => actions.remove_line(),
        }
    };

    let mut command = build_env
        .cargo()
        .args(args)
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "full")
        .env(rustflags_env, rustflags);

    if check_errors {
        command = command.process_lines(&mut detect_error);
    }

    if ctx.quiet {
        command = command.no_output_timeout(None);
    }

    match command.run() {
        Ok(()) => Ok(()),
        Err(e) => {
            if did_ice {
                Err(e.context(FailureReason::ICE).into())
            } else if !deps.is_empty() {
                Err(e.context(FailureReason::DependsOn(deps)).into())
            } else if !error_codes.is_empty() {
                Err(e.context(FailureReason::CompilerError(error_codes)).into())
            } else {
                Err(e.into())
            }
        }
    }
}

pub(super) fn run_test<DB: WriteResults>(
    action: &str,
    ctx: &TaskCtx<DB>,
    test_fn: fn(&TaskCtx<DB>, &Build, &HashSet<PackageId>) -> Fallible<TestResult>,
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
            .get(ctx.krate)
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
                    .cpu_limit(Some(f32::from(ctx.config.sandbox.cpu_limit)))
                    .enable_networking(false);

                let krate = &ctx.krate.to_rustwide();
                let mut build_dir = ctx.build_dir.lock().unwrap();
                let mut build = build_dir.build(ctx.toolchain, krate, sandbox);

                for patch in ctx.toolchain.patches.iter() {
                    build = build.patch_with_git(&patch.name, &patch.repo, &patch.branch);
                }

                detect_broken(build.run(|build| {
                    let local_packages_id = get_local_packages(build)?;
                    test_fn(ctx, build, &local_packages_id)
                }))
            },
        )?;
    }
    Ok(())
}

fn build<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
    local_packages_id: &HashSet<PackageId>,
) -> Fallible<()> {
    let cpus = ctx.config.sandbox.cpu_limit.to_string();
    run_cargo(
        ctx,
        build_env,
        &[
            "build",
            "--frozen",
            "--message-format=json",
            "--jobs",
            &cpus,
        ],
        true,
        local_packages_id,
    )?;
    run_cargo(
        ctx,
        build_env,
        &[
            "test",
            "--frozen",
            "--no-run",
            "--message-format=json",
            "--jobs",
            &cpus,
        ],
        true,
        local_packages_id,
    )?;
    Ok(())
}

fn test<DB: WriteResults>(ctx: &TaskCtx<DB>, build_env: &Build) -> Fallible<()> {
    let cpus = ctx.config.sandbox.cpu_limit.to_string();
    run_cargo(
        ctx,
        build_env,
        &[
            "test",
            "--frozen",
            "--jobs",
            &cpus,
            "--",
            "--test-threads",
            &cpus,
        ],
        false,
        &HashSet::new(),
    )
}

pub(super) fn test_build_and_test<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
    local_packages_id: &HashSet<PackageId>,
) -> Fallible<TestResult> {
    let build_r = build(ctx, build_env, local_packages_id);
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
    local_packages_id: &HashSet<PackageId>,
) -> Fallible<TestResult> {
    if let Err(err) = build(ctx, build_env, local_packages_id) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestSkipped)
    }
}

pub(super) fn test_check_only<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
    local_packages_id: &HashSet<PackageId>,
) -> Fallible<TestResult> {
    let cpus = ctx.config.sandbox.cpu_limit.to_string();
    if let Err(err) = run_cargo(
        ctx,
        build_env,
        &[
            "check",
            "--frozen",
            "--all",
            "--all-targets",
            "--message-format=json",
            "--jobs",
            &cpus,
        ],
        true,
        local_packages_id,
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub(super) fn test_clippy_only<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
    local_packages_id: &HashSet<PackageId>,
) -> Fallible<TestResult> {
    let cpus = ctx.config.sandbox.cpu_limit.to_string();
    if let Err(err) = run_cargo(
        ctx,
        build_env,
        &[
            "clippy",
            "--frozen",
            "--all",
            "--all-targets",
            "--message-format=json",
            "--jobs",
            &cpus,
        ],
        true,
        local_packages_id,
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub(super) fn test_rustdoc<DB: WriteResults>(
    ctx: &TaskCtx<DB>,
    build_env: &Build,
    local_packages_id: &HashSet<PackageId>,
) -> Fallible<TestResult> {
    let cpus = ctx.config.sandbox.cpu_limit.to_string();
    let res = run_cargo(
        ctx,
        build_env,
        &[
            "doc",
            "--frozen",
            "--no-deps",
            "--document-private-items",
            "--message-format=json",
            "--jobs",
            &cpus,
        ],
        true,
        local_packages_id,
    );

    // Make sure to remove the built documentation
    // There is no point in storing it after the build is done
    remove_dir_all(&build_env.host_target_dir().join("doc"))?;

    if let Err(err) = res {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}
