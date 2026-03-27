//! Cargo invocation and result classification for each experiment mode.

use crate::crates::Crate;
use crate::experiments::CapLints;
use crate::prelude::*;
use crate::results::DiagnosticCode;
use crate::results::{BrokenReason, FailureReason, TestResult};
use crate::runner::tasks::TaskCtx;
use crate::runner::OverrideResult;
use anyhow::Error;
use cargo_metadata::diagnostic::DiagnosticLevel;
use cargo_metadata::{CrateType, Metadata, Package, Target, TargetKind};
use docsrs_metadata::Metadata as DocsrsMetadata;
use remove_dir_all::remove_dir_all;
use rustwide::cmd::{CommandError, MountKind, ProcessLinesActions, SandboxBuilder};
use rustwide::logging::LogStorage;
use rustwide::{Build, PrepareError};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::ErrorKind;

/// Maps an error into a [`FailureReason`] by inspecting its downcast chain.
// - Checks for a directly attached FailureReason first.
// - Then inspects CommandError variants for timeout, OOM, docker, and IO errors.
// - Falls back to FailureReason::Unknown.
pub(crate) fn failure_reason(err: &Error) -> FailureReason {
    if let Some(reason) = err.downcast_ref::<FailureReason>() {
        reason.clone()
    } else if let Some(command_error) = err.downcast_ref::<CommandError>() {
        match command_error {
            CommandError::NoOutputFor(_)
            | CommandError::Timeout(_)
            | CommandError::KillAfterTimeoutFailed(_) => FailureReason::Timeout,
            CommandError::SandboxOOM => FailureReason::OOM,
            CommandError::SandboxImagePullFailed(_)
            | CommandError::SandboxImageMissing(_)
            | CommandError::SandboxContainerCreate(_)
            | CommandError::WorkspaceNotMountedCorrectly
            | CommandError::InvalidDockerInspectOutput(_) => FailureReason::Docker,
            CommandError::IO(io) => match io.kind() {
                ErrorKind::OutOfMemory => FailureReason::OOM,
                ErrorKind::StorageFull | ErrorKind::QuotaExceeded | ErrorKind::TooManyLinks => {
                    FailureReason::NoSpace
                }
                _ => FailureReason::Unknown,
            },
            CommandError::ExecutionFailed { .. } | _ => FailureReason::Unknown,
        }
    } else {
        FailureReason::Unknown
    }
}

/// Converts known preparation errors into [`OverrideResult`] with a broken-crate reason.
// - On Ok, passes through unchanged.
// - On Err, downcasts to PrepareError and maps known variants (missing
//   Cargo.toml, yanked deps, private git repos, etc.) to BrokenReason.
// - Wraps matched errors in OverrideResult so callers record BrokenCrate
//   instead of a generic failure.
pub(super) fn detect_broken<T>(res: Result<T, Error>) -> Result<T, Error> {
    match res {
        Ok(ok) => Ok(ok),
        Err(err) => {
            if let Some(error) = err.downcast_ref() {
                let reason = match *error {
                    PrepareError::MissingCargoToml => {
                        TestResult::BrokenCrate(BrokenReason::CargoToml)
                    }
                    PrepareError::InvalidCargoTomlSyntax => {
                        TestResult::BrokenCrate(BrokenReason::CargoToml)
                    }
                    PrepareError::BrokenDependencies(_) => {
                        TestResult::BrokenCrate(BrokenReason::BrokenDependencies)
                    }
                    PrepareError::YankedDependencies(_) => {
                        TestResult::BrokenCrate(BrokenReason::Yanked)
                    }
                    PrepareError::MissingDependencies(_) => {
                        TestResult::BrokenCrate(BrokenReason::MissingDependencies)
                    }
                    PrepareError::InvalidCargoLock(_) => {
                        TestResult::BrokenCrate(BrokenReason::InvalidCargoLock)
                    }
                    PrepareError::PrivateGitRepository => {
                        TestResult::BrokenCrate(BrokenReason::MissingGitRepository)
                    }
                    _ => {
                        let reason = failure_reason(&err);
                        if reason.is_spurious() || matches!(reason, FailureReason::Unknown) {
                            TestResult::PrepareFail(reason)
                        } else {
                            TestResult::BrokenCrate(BrokenReason::Unknown)
                        }
                    }
                };

                Err(err.context(OverrideResult(reason)))
            } else {
                Err(err)
            }
        }
    }
}

/// Runs `cargo metadata` to discover which packages live in the crate's source tree.
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

/// Assembles and runs a cargo command inside the sandbox, classifying its output.
// - Appends --target and toolchain-specific cargo/rust flags to the argument list.
// - Builds RUSTFLAGS and RUSTDOCFLAGS from cap_lints and toolchain overrides.
// - When check_errors is true, attaches a JSON line processor that scans for:
//     - ICEs (internal compiler errors) in local or dependency crates.
//     - Error codes from the crate under test vs. its dependencies.
//     - Network access, OOM, disk-full, and trybuild diagnostic changes.
//   Replaces raw JSON lines with rendered diagnostics for human-readable logs.
// - Sets CARGO_INCREMENTAL=0 and RUST_BACKTRACE=full, plus caller-supplied env.
// - Disables the output timeout for quiet crates.
// - On failure, wraps the error with the most specific detected FailureReason
//   (ICE > NoSpace > DependsOn > CompilerError > NetworkAccess > DiagnosticChange).
fn run_cargo(
    ctx: &TaskCtx,
    build_env: &Build,
    args: &[&str],
    check_errors: bool,
    local_packages: &[Package],
    env: HashMap<&'static str, String>,
    mount_kind: MountKind,
    cap_lints: Option<CapLints>,
) -> Fallible<()> {
    let local_packages_id: HashSet<_> = local_packages.iter().map(|p| &p.id).collect();

    let mut args = args.to_vec();
    if let Some(ref target) = ctx.toolchain.target {
        args.extend(["--target", target]);
    }
    if let Some(ref tc_cargoflags) = ctx.toolchain.cargoflags {
        args.extend(tc_cargoflags.split(' '));
    }

    let mut rustflags = cap_lints
        .map(|cap| format!("--cap-lints={cap}"))
        .unwrap_or_default();
    if let Some(ref tc_rustflags) = ctx.toolchain.rustflags {
        rustflags.push(' ');
        rustflags.push_str(tc_rustflags);
    }

    let mut rustdocflags = cap_lints
        .map(|cap| format!("--cap-lints={cap}"))
        .unwrap_or_default();
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
        if line.contains("collect2: fatal error: ld terminated with signal 7 [Bus error]") {
            // the cause of the bus error is running out of disk space
            ran_out_of_space = true;
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
        .source_dir_mount_kind(mount_kind)
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

/// A cargo message
///
/// This duplicates cargo_metadata::Message, but without variants we don't need to avoid recursion
/// issues. See https://github.com/rust-lang/rust/issues/152354 for context.
#[derive(Debug, Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
enum Message {
    /// The compiler wants to display a message
    CompilerMessage(CompilerMessage),
    #[serde(other)]
    Other,
}

/// A cargo message
///
/// This duplicates cargo_metadata::CompilerMessage, but without variants we don't need to avoid recursion
/// issues. See https://github.com/rust-lang/rust/issues/152354 for context.
#[derive(Debug, Deserialize)]
struct CompilerMessage {
    /// The package this message belongs to
    package_id: cargo_metadata::PackageId,
    /// The message the compiler sent.
    message: Diagnostic,
}

/// A diagnostic message generated by rustc
///
/// This duplicates cargo_metadata::Diagnostic, but without variants we don't need to avoid recursion
/// issues. See https://github.com/rust-lang/rust/issues/152354 for context.
#[derive(Debug, Deserialize)]
struct Diagnostic {
    /// The associated error code for this diagnostic
    code: Option<cargo_metadata::diagnostic::DiagnosticCode>,
    /// "error: internal compiler error", "error", "warning", "note", "help"
    level: cargo_metadata::diagnostic::DiagnosticLevel,
    /// The message as rustc would render it
    rendered: Option<String>,
}

/// Sets up the sandbox, locks the build directory, and runs the given test function.
// - Captures all output into the provided LogStorage.
// - Creates a sandboxed environment with a memory limit and no networking.
// - Locks the per-toolchain BuildDirectory and starts a build with any
//   toolchain patches applied.
// - Calls get_local_packages to discover crates in the source tree, then
//   invokes the mode-specific test_fn.
// - Wraps the whole thing in detect_broken to catch preparation failures.
pub(super) fn run_test(
    action: &str,
    ctx: &TaskCtx,
    test_fn: fn(&TaskCtx, &Build, &[Package]) -> Fallible<TestResult>,
    logs: &LogStorage,
) -> Fallible<TestResult> {
    rustwide::logging::capture(logs, || {
        info!(
            "{} {} against {} for {}",
            action, ctx.krate, ctx.toolchain, ctx.experiment.name
        );
        let sandbox = SandboxBuilder::new()
            .memory_limit(Some(ctx.config.sandbox.memory_limit.to_bytes()))
            .enable_networking(false);

        let krate = &ctx.krate.to_rustwide();
        let mut build_dir = ctx.build_dir.lock().unwrap();
        let mut build = build_dir.build(ctx.toolchain, krate, sandbox);

        for patch in ctx.toolchain.patches.iter() {
            build = build.patch_with_git(&patch.name, patch.repo.as_str(), &patch.branch);
        }

        detect_broken(build.run(|build| {
            let local_packages = get_local_packages(build)?;
            test_fn(ctx, build, &local_packages)
        }))
    })
}

/// Compiles the crate and its test binaries (without executing them).
// - Runs `cargo build --frozen` with JSON diagnostics.
// - Runs `cargo test --frozen --no-run` to compile test harnesses.
fn run_cargo_build(ctx: &TaskCtx, build_env: &Build, local_packages: &[Package]) -> Fallible<()> {
    run_cargo(
        ctx,
        build_env,
        &["build", "--frozen", "--message-format=json"],
        true,
        local_packages,
        HashMap::default(),
        MountKind::ReadOnly,
        Some(ctx.experiment.cap_lints),
    )?;
    run_cargo(
        ctx,
        build_env,
        &["test", "--frozen", "--no-run", "--message-format=json"],
        true,
        local_packages,
        HashMap::default(),
        MountKind::ReadOnly,
        Some(ctx.experiment.cap_lints),
    )?;
    Ok(())
}

/// Executes the compiled test binaries via `cargo test --frozen`.
fn run_cargo_test(ctx: &TaskCtx, build_env: &Build) -> Fallible<()> {
    run_cargo(
        ctx,
        build_env,
        &["test", "--frozen"],
        false,
        &[],
        HashMap::default(),
        MountKind::ReadOnly,
        Some(ctx.experiment.cap_lints),
    )
}

/// Builds the crate and runs its test suite, returning the combined result.
// - Calls build(); if that fails, returns BuildFail immediately.
// - On successful build, calls test(); maps its result to TestFail or TestPass.
pub(super) fn test_build_and_test(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages_id: &[Package],
) -> Fallible<TestResult> {
    let build_r = run_cargo_build(ctx, build_env, local_packages_id);
    let test_r = if build_r.is_ok() {
        Some(run_cargo_test(ctx, build_env))
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

/// Builds the crate without running tests.
pub(super) fn test_build_only(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages_id: &[Package],
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo_build(ctx, build_env, local_packages_id) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestSkipped)
    }
}

/// Runs `cargo check` on the crate.
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
        MountKind::ReadOnly,
        Some(ctx.experiment.cap_lints),
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

/// Runs `cargo clippy` on the crate.
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
        MountKind::ReadOnly,
        Some(ctx.experiment.cap_lints),
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

/// Runs `cargo doc` (and docs.rs metadata if applicable) on the crate.
// - First runs a standard `cargo doc --no-deps --document-private-items`.
// - Cleans up the generated docs directory after each run to save disk.
// - If the crate contains a library target, re-runs with docs.rs metadata
//   (custom features, rustdoc args) and RUSTC_BOOTSTRAP=1.
pub(super) fn test_rustdoc(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages: &[Package],
) -> Fallible<TestResult> {
    let run = |cargo_args, env| {
        let res = run_cargo(
            ctx,
            build_env,
            cargo_args,
            true,
            local_packages,
            env,
            MountKind::ReadOnly,
            Some(ctx.experiment.cap_lints),
        );

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
    target.crate_types.iter().any(|ty| *ty != CrateType::Bin)
        && target
            .kind
            .iter()
            .all(|k| ![TargetKind::Example, TargetKind::Test, TargetKind::Bench].contains(k))
}

/// Runs `cargo fix` on the crate.
pub(crate) fn fix(
    ctx: &TaskCtx,
    build_env: &Build,
    local_packages_id: &[Package],
) -> Fallible<TestResult> {
    if let Err(err) = run_cargo(
        ctx,
        build_env,
        &[
            "fix",
            "--allow-no-vcs",
            "--allow-dirty",
            "--frozen",
            "--all",
            "--all-targets",
            "--message-format=json",
        ],
        true,
        local_packages_id,
        HashMap::default(),
        MountKind::ReadWrite,
        None,
    ) {
        Ok(TestResult::BuildFail(failure_reason(&err)))
    } else {
        Ok(TestResult::TestPass)
    }
}

#[test]
fn test_failure_reason() {
    let error: anyhow::Error = anyhow!(CommandError::IO(std::io::Error::other("Test")));
    assert_eq!(failure_reason(&error), FailureReason::Unknown);
    assert_eq!(
        failure_reason(&error.context(FailureReason::ICE)),
        FailureReason::ICE
    );
}
