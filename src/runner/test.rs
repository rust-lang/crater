use crate::crates::Crate;
use crate::prelude::*;
use crate::results::DiagnosticCode;
use crate::results::{BrokenReason, FailureReason, TestResult};
use crate::runner::tasks::TaskCtx;
use crate::runner::OverrideResult;
use anyhow::Error;
use cargo_metadata::diagnostic::DiagnosticLevel;
use cargo_metadata::{Message, Metadata, Package, Target};
use docsrs_metadata::Metadata as DocsrsMetadata;
use remove_dir_all::remove_dir_all;
use rustwide::cmd::{CommandError, ProcessLinesActions, SandboxBuilder};
use rustwide::logging::LogStorage;
use rustwide::{Build, PrepareError};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::ErrorKind;

fn failure_reason(err: &Error) -> FailureReason {
    for cause in err.chain() {
        if let Some(&CommandError::SandboxOOM) = cause.downcast_ref() {
            return FailureReason::OOM;
        } else if let Some(&CommandError::NoOutputFor(_) | &CommandError::Timeout(_)) =
            cause.downcast_ref()
        {
            return FailureReason::Timeout;
        } else if let Some(reason) = cause.downcast_ref::<FailureReason>() {
            return reason.clone();
        } else if let Some(CommandError::IO(io)) = cause.downcast_ref() {
            match io.kind() {
                ErrorKind::OutOfMemory => {
                    return FailureReason::OOM;
                }
                _ => {
                    // FIXME use ErrorKind once #![feature(io_error_more)] is stable <https://github.com/rust-lang/rust/issues/86442>
                    #[cfg(target_os = "linux")]
                    match io.raw_os_error() {
                        // <https://mariadb.com/kb/en/operating-system-error-codes/#linux-error-codes>
                        | Some(28) /* ErrorKind::StorageFull */
                        | Some(122) /* ErrorKind::FilesystemQuotaExceeded */
                        | Some(31) /* TooManyLinks */=> {
                            return FailureReason::NoSpace
                        }
                        _ => {}
                    }

                    #[cfg(target_os = "windows")]
                    match io.raw_os_error() {
                        // <https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes>
                        | Some(39|112) /* ErrorKind::StorageFull */
                        | Some(1295) /* ErrorKind::FilesystemQuotaExceeded */
                        | Some(1142) /* TooManyLinks */=> {
                            return FailureReason::NoSpace
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    FailureReason::Unknown
}

pub(super) fn detect_broken<T>(res: Result<T, Error>) -> Result<T, Error> {
    match res {
        Ok(ok) => Ok(ok),
        Err(err) => {
            let mut reason = None;
            for cause in err.chain() {
                if let Some(error) = cause.downcast_ref() {
                    reason = match *error {
                        PrepareError::MissingCargoToml => Some(BrokenReason::CargoToml),
                        PrepareError::InvalidCargoTomlSyntax => Some(BrokenReason::CargoToml),
                        PrepareError::YankedDependencies(_) => Some(BrokenReason::Yanked),
                        PrepareError::MissingDependencies(_) => {
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
                Err(err.context(OverrideResult(TestResult::BrokenCrate(reason))))
            } else {
                Err(err)
            }
        }
    }
}

fn get_local_packages(build_env: &Build) -> Fallible<Vec<Package>> {
    Ok(build_env
        .cargo()
        .args(&["metadata", "--no-deps", "--format-version=1"])
        .log_output(false)
        .run_capture()?
        .stdout_lines()
        .iter()
        .filter_map(|line| serde_json::from_str::<Metadata>(line).ok())
        .flat_map(|metadata| metadata.packages)
        .collect())
}

fn run_cargo(
    ctx: &TaskCtx,
    build_env: &Build,
    args: &[&str],
    check_errors: bool,
    local_packages: &[Package],
    env: HashMap<&'static str, String>,
) -> Fallible<()> {
    let local_packages_id: HashSet<_> = local_packages.iter().map(|p| &p.id).collect();

    let mut args = args.to_vec();
    if let Some(ref target) = ctx.toolchain.target {
        args.extend(["--target", target]);
    }
    if let Some(ref tc_cargoflags) = ctx.toolchain.cargoflags {
        args.extend(tc_cargoflags.split(' '));
    }

    let mut rustflags = format!("--cap-lints={}", ctx.experiment.cap_lints.to_str());
    if let Some(ref tc_rustflags) = ctx.toolchain.rustflags {
        rustflags.push(' ');
        rustflags.push_str(tc_rustflags);
    }

    let mut rustdocflags = format!("--cap-lints={}", ctx.experiment.cap_lints.to_str());
    if let Some(ref tc_rustdocflags) = ctx.toolchain.rustdocflags {
        rustdocflags.push(' ');
        rustdocflags.push_str(tc_rustdocflags);
    }

    let mut did_ice = false;
    let mut did_network = false;
    let mut did_trybuild = false;
    let mut ran_out_of_space = false;
    let mut error_codes = BTreeSet::new();
    let mut deps = BTreeSet::new();

    let mut detect_error = |line: &str, actions: &mut ProcessLinesActions| {
        if line.contains("urlopen error") && line.contains("Temporary failure in name resolution") {
            did_network = true;
        }
        if line.contains("Address already in use") {
            did_network = true;
        }
        if line.to_lowercase().contains("no space left on device") {
            ran_out_of_space = true;
        }
        if line.contains("code: 111") && line.contains("Connection refused") {
            did_network = true;
        }
        if line.contains("the environment variable TRYBUILD=overwrite") {
            did_trybuild = true;
        }

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
        .args(&args)
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "full")
        .env("RUSTFLAGS", rustflags)
        .env("RUSTDOCFLAGS", rustdocflags);
    for (var, data) in env {
        command = command.env(var, data);
    }

    if check_errors {
        command = command.process_lines(&mut detect_error);
    }

    if ctx.quiet {
        command = command.no_output_timeout(None);
    }

    match command.run() {
        Ok(()) => Ok(()),
        e @ Err(_) => {
            if did_ice {
                e.context(FailureReason::ICE)
            } else if ran_out_of_space {
                e.context(FailureReason::NoSpace)
            } else if !deps.is_empty() {
                e.context(FailureReason::DependsOn(deps))
            } else if !error_codes.is_empty() {
                e.context(FailureReason::CompilerError(error_codes))
            } else if did_network {
                e.context(FailureReason::NetworkAccess)
            } else if did_trybuild {
                e.context(FailureReason::CompilerDiagnosticChange)
            } else {
                e.map_err(|err| err.into())
            }
        }
    }
}

pub(super) fn run_test(
    action: &str,
    ctx: &TaskCtx,
    test_fn: fn(&TaskCtx, &Build, &[Package]) -> Fallible<TestResult>,
    logs: &LogStorage,
) -> Fallible<TestResult> {
    rustwide::logging::capture(logs, || {
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

        let krate = &ctx.krate.to_rustwide();
        let mut build_dir = ctx.build_dir.lock().unwrap();
        let mut build = build_dir.build(ctx.toolchain, krate, sandbox);

        for patch in ctx.toolchain.patches.iter() {
            build = build.patch_with_git(&patch.name, &patch.repo, &patch.branch);
        }

        detect_broken(build.run(|build| {
            let local_packages = get_local_packages(build)?;
            test_fn(ctx, build, &local_packages)
        }))
    })
}

fn build(ctx: &TaskCtx, build_env: &Build, local_packages: &[Package]) -> Fallible<()> {
    run_cargo(
        ctx,
        build_env,
        &["build", "--frozen", "--message-format=json"],
        true,
        local_packages,
        HashMap::default(),
    )?;
    run_cargo(
        ctx,
        build_env,
        &["test", "--frozen", "--no-run", "--message-format=json"],
        true,
        local_packages,
        HashMap::default(),
    )?;
    Ok(())
}

fn test(ctx: &TaskCtx, build_env: &Build) -> Fallible<()> {
    run_cargo(
        ctx,
        build_env,
        &["test", "--frozen"],
        false,
        &[],
        HashMap::default(),
    )
}

pub(super) fn test_build_and_test(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages_id: &[Package],
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

pub(super) fn test_build_only(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages_id: &[Package],
) -> Fallible<TestResult> {
    if let Err(err) = build(ctx, build_env, local_packages_id) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestSkipped)
    }
}

pub(super) fn test_check_only(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages_id: &[Package],
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        ctx,
        build_env,
        &[
            "check",
            "--frozen",
            "--all",
            "--all-targets",
            "--message-format=json",
        ],
        true,
        local_packages_id,
        HashMap::default(),
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub(super) fn test_clippy_only(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages: &[Package],
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        ctx,
        build_env,
        &[
            "clippy",
            "--frozen",
            "--all",
            "--all-targets",
            "--message-format=json",
        ],
        true,
        local_packages,
        HashMap::default(),
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

pub(super) fn test_rustdoc(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages: &[Package],
) -> Fallible<TestResult> {
    let run = |cargo_args, env| {
        let res = run_cargo(ctx, build_env, cargo_args, true, local_packages, env);

        // Make sure to remove the built documentation
        // There is no point in storing it after the build is done
        remove_dir_all(build_env.host_target_dir().join("doc"))?;

        res
    };

    // first, run a normal `cargo doc`
    let res = run(
        &[
            "doc",
            "--frozen",
            "--no-deps",
            "--document-private-items",
            "--message-format=json",
        ],
        HashMap::default(),
    );
    if let Err(err) = res {
        return Ok(TestResult::BuildFail(failure_reason(&err)));
    }

    // next, if this is a library, run it with docs.rs metadata applied.
    if local_packages
        .iter()
        .any(|p| p.targets.iter().any(is_library))
    {
        let src = build_env.host_source_dir();
        let metadata = DocsrsMetadata::from_crate_root(src)?;
        let cargo_args = metadata.cargo_args(
            &["--frozen".into(), "--message-format=json".into()],
            &["--document-private-items".into()],
        );
        assert_eq!(cargo_args[0], "rustdoc");
        let cargo_args: Vec<_> = cargo_args.iter().map(|s| s.as_str()).collect();
        let mut env = metadata.environment_variables();
        // docsrs-metadata requires a nightly environment, but crater sometimes runs tests on beta and
        // stable.
        env.insert("RUSTC_BOOTSTRAP", "1".to_string());

        if let Err(err) = run(&cargo_args, env) {
            return Ok(TestResult::BuildFail(failure_reason(&err)));
        }
    }

    Ok(TestResult::TestPass)
}

fn is_library(target: &Target) -> bool {
    // Some examples and tests can be libraries (e.g. if they use `cdylib`).
    target.crate_types.iter().any(|ty| ty != "bin")
        && target
            .kind
            .iter()
            .all(|k| !["example", "test", "bench"].contains(&k.as_str()))
}


#[test]
fn test_failure_reason() {
    let error : anyhow::Error = anyhow!(CommandError::IO(std::io::Error::other("Test")));
    assert_eq!(failure_reason(&error), FailureReason::Unknown);
    assert_eq!(failure_reason(&error.context(FailureReason::ICE)), FailureReason::ICE);
}
