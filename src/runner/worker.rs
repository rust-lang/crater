use crate::agent::AgentApi;
use crate::crates::Crate;
use crate::experiments::{Experiment, Mode};
use crate::prelude::*;
use crate::results::{BrokenReason, TestResult};
use crate::runner::tasks::{Task, TaskStep};
use crate::runner::test::{detect_broken, failure_reason};
use crate::runner::OverrideResult;
use crate::toolchain::Toolchain;
use crate::utils;
use rustwide::logging::{self, LogStorage};
use rustwide::{BuildDirectory, Workspace};
use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::Duration;

pub trait RecordProgress: Send + Sync {
    fn record_progress(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        log: &[u8],
        result: &TestResult,
        version: Option<(&Crate, &Crate)>,
    ) -> Fallible<()>;
}

impl RecordProgress for AgentApi {
    fn record_progress(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        log: &[u8],
        result: &TestResult,
        version: Option<(&Crate, &Crate)>,
    ) -> Fallible<()> {
        self.record_progress(ex, krate, toolchain, log, result, version)
    }
}

pub(super) struct Worker<'a> {
    name: String,
    workspace: &'a Workspace,
    build_dir: HashMap<&'a crate::toolchain::Toolchain, Mutex<BuildDirectory>>,
    ex: &'a Experiment,
    config: &'a crate::config::Config,
    api: &'a dyn RecordProgress,
    target_dir_cleanup: AtomicBool,
    next_crate: &'a (dyn Fn() -> Fallible<Option<Crate>> + Send + Sync),
}

impl<'a> Worker<'a> {
    pub(super) fn new(
        name: String,
        workspace: &'a Workspace,
        ex: &'a Experiment,
        config: &'a crate::config::Config,
        api: &'a dyn RecordProgress,
        next_crate: &'a (dyn Fn() -> Fallible<Option<Crate>> + Send + Sync),
    ) -> Self {
        let mut build_dir = HashMap::new();
        build_dir.insert(
            &ex.toolchains[0],
            Mutex::new(workspace.build_dir(&format!("{name}-tc1"))),
        );
        build_dir.insert(
            &ex.toolchains[1],
            Mutex::new(workspace.build_dir(&format!("{name}-tc2"))),
        );
        Worker {
            build_dir,
            name,
            workspace,
            ex,
            config,
            next_crate,
            api,
            target_dir_cleanup: AtomicBool::new(false),
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    fn run_task(
        &self,
        task: &Task,
        storage: &LogStorage,
    ) -> Result<TestResult, (anyhow::Error, TestResult)> {
        info!("running task: {task:?}");

        let mut res = None;
        let max_attempts = 5;
        for run in 1..=max_attempts {
            // If we're running a task, we call ourselves healthy.
            crate::agent::set_healthy();

            match task.run(self.config, &self.build_dir, self.ex, storage) {
                Ok(res) => return Ok(res),
                Err(e) => {
                    res = Some(e);
                }
            }

            // We retry task failing on the second toolchain (i.e., regressions). In
            // the future we might expand this list further but for now this helps
            // prevent spurious test failures and such.
            //
            // For now we make no distinction between build failures and test failures
            // here, but that may change if this proves too slow.
            let mut should_retry = false;
            if self.ex.toolchains.len() == 2 {
                let toolchain = match &task.step {
                    TaskStep::BuildAndTest { tc, .. }
                    | TaskStep::BuildOnly { tc, .. }
                    | TaskStep::CheckOnly { tc, .. }
                    | TaskStep::Clippy { tc, .. }
                    | TaskStep::Rustdoc { tc, .. }
                    | TaskStep::UnstableFeatures { tc }
                    | TaskStep::Fix { tc, .. } => Some(tc),
                };
                if let Some(toolchain) = toolchain {
                    if toolchain == self.ex.toolchains.last().unwrap() {
                        should_retry = true;
                    }
                }
            }

            if !should_retry {
                break;
            }

            log::info!("Retrying task {task:?} [{run}/{max_attempts}]");
        }
        // Unreachable unless we failed to succeed above.
        let e = res.unwrap();
        error!("task {task:?} failed");
        utils::report_failure(&e);

        let mut result = if self.config.is_broken(&task.krate) {
            TestResult::BrokenCrate(BrokenReason::Unknown)
        } else {
            TestResult::Error
        };

        if let Some(OverrideResult(res)) = e.downcast_ref() {
            result = res.clone();
        }

        Err((e, result))
    }

    pub(super) fn run(&self) -> Fallible<()> {
        loop {
            let krate = if let Some(next) = (self.next_crate)()? {
                next
            } else {
                // We're done if no more crates left.
                return Ok(());
            };

            self.maybe_cleanup_target_dir()?;

            info!("{} processing crate {}", self.name, krate);

            if !self.ex.ignore_blacklist && self.config.should_skip(&krate) {
                for tc in &self.ex.toolchains {
                    // If a skipped crate is somehow sent to the agent (for example, when a crate was
                    // added to the experiment and *then* blacklisted) report the crate as skipped
                    // instead of silently ignoring it.
                    if let Err(e) = self.api.record_progress(
                        self.ex,
                        &krate,
                        tc,
                        "crate skipped".as_bytes(),
                        &TestResult::Skipped,
                        None,
                    ) {
                        crate::utils::report_failure(&e);
                    }
                }
                continue;
            }

            let mut updated_version = None;
            let logs = LogStorage::from(self.config);
            let prepare = logging::capture(&logs, || {
                let rustwide_crate = krate.to_rustwide();
                for attempt in 1..=15 {
                    match detect_broken(rustwide_crate.fetch(self.workspace)) {
                        Ok(()) => break,
                        Err(e) => {
                            if logs.to_string().contains("No space left on device") {
                                if attempt == 15 {
                                    // If we've failed 15 times, then
                                    // just give up. It's been at least
                                    // 45 seconds, which is enough that
                                    // our disk space check should
                                    // have run at least once in this
                                    // time. If that's not helped, then
                                    // maybe this git repository *is*
                                    // actually too big.
                                    //
                                    // Ideally we'd have some kind of
                                    // per-worker counter and if we hit
                                    // this too often we'd replace the
                                    // machine, but it's not very clear
                                    // what "too often" means here.
                                    return Err(e);
                                } else {
                                    log::warn!(
                                        "Retrying crate fetch in 3 seconds (attempt {attempt})"
                                    );
                                    std::thread::sleep(std::time::Duration::from_secs(3));
                                }
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }

                if let Crate::GitHub(repo) = &krate {
                    if let Some(sha) = rustwide_crate.git_commit(self.workspace) {
                        let updated = crate::crates::GitHubRepo {
                            sha: Some(sha),
                            ..repo.clone()
                        };
                        updated_version = Some(Crate::GitHub(updated));
                    } else {
                        bail!("unable to capture sha for {}", repo.slug());
                    }
                }
                Ok(())
            });
            if let Err(err) = prepare {
                let mut result = if self.config.is_broken(&krate) {
                    TestResult::BrokenCrate(BrokenReason::Unknown)
                } else {
                    TestResult::PrepareFail(failure_reason(&err))
                };

                if let Some(OverrideResult(res)) = err.downcast_ref() {
                    result = res.clone();
                }

                for tc in &self.ex.toolchains {
                    if let Err(e) = self.api.record_progress(
                        self.ex,
                        &krate,
                        tc,
                        format!("{logs}\n\nthis task or one of its parent failed: {err:?}")
                            .as_bytes(),
                        &result,
                        updated_version.as_ref().map(|new| (&krate, new)),
                    ) {
                        crate::utils::report_failure(&e);
                    }
                }
                continue;
            }

            for tc in &self.ex.toolchains {
                let quiet = self.config.is_quiet(&krate);
                let task = Task {
                    krate: krate.clone(),
                    step: match self.ex.mode {
                        Mode::BuildOnly => TaskStep::BuildOnly {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::BuildAndTest
                            if !self.ex.ignore_blacklist
                                && self.config.should_skip_tests(&krate) =>
                        {
                            TaskStep::BuildOnly {
                                tc: tc.clone(),
                                quiet,
                            }
                        }
                        Mode::BuildAndTest => TaskStep::BuildAndTest {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::CheckOnly => TaskStep::CheckOnly {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::Clippy => TaskStep::Clippy {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::Rustdoc => TaskStep::Rustdoc {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::UnstableFeatures => TaskStep::UnstableFeatures { tc: tc.clone() },
                        Mode::Fix => TaskStep::Fix {
                            tc: tc.clone(),
                            quiet,
                        },
                    },
                };

                // Fork logs off to distinct branch, so that each toolchain has its own log file,
                // while keeping the shared prepare step in common.
                let storage = logs.duplicate();
                match self.run_task(&task, &storage) {
                    Ok(res) => {
                        self.api.record_progress(
                            self.ex,
                            &task.krate,
                            tc,
                            storage.to_string().as_bytes(),
                            &res,
                            updated_version.as_ref().map(|new| (&krate, new)),
                        )?;
                    }
                    Err((err, test_result)) => {
                        self.api.record_progress(
                            self.ex,
                            &task.krate,
                            tc,
                            format!("{storage}\n\n{err:?}").as_bytes(),
                            &test_result,
                            updated_version.as_ref().map(|new| (&krate, new)),
                        )?;
                    }
                }
            }
        }
    }

    fn maybe_cleanup_target_dir(&self) -> Fallible<()> {
        if !self.target_dir_cleanup.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        info!("purging target dir for {}", self.name);
        for dir in self.build_dir.values() {
            dir.lock().unwrap().purge()?;
        }
        Ok(())
    }

    fn schedule_target_dir_cleanup(&self) {
        self.target_dir_cleanup.store(true, Ordering::SeqCst);
    }
}

pub(super) struct DiskSpaceWatcher<'a> {
    interval: Duration,
    threshold: f32,
    workers: &'a [Worker<'a>],
    should_stop: Mutex<bool>,
    waiter: Condvar,
}

impl<'a> DiskSpaceWatcher<'a> {
    pub(super) fn new(interval: Duration, threshold: f32, workers: &'a [Worker<'a>]) -> Self {
        DiskSpaceWatcher {
            interval,
            threshold,
            workers,
            should_stop: Mutex::new(false),
            waiter: Condvar::new(),
        }
    }

    pub(super) fn stop(&self) {
        *self.should_stop.lock().unwrap() = true;
        self.waiter.notify_all();
    }

    pub(super) fn run(&self) {
        let mut should_stop = self.should_stop.lock().unwrap();
        while !*should_stop {
            self.check();
            // Wait for either the interval to pass or should_stop to get a
            // write. We don't care if we timed out or not, we can double check
            // the should_stop regardless.
            should_stop = self
                .waiter
                .wait_timeout(should_stop, self.interval)
                .unwrap()
                .0;
        }
    }

    fn check(&self) {
        let usage = match crate::utils::disk_usage::DiskUsage::fetch() {
            Ok(usage) => usage,
            Err(err) => {
                // TODO: `current_mount` fails sometimes on Windows with ERROR_DEVICE_NOT_READY.
                warn!("Failed to check space remaining: {err}");
                return;
            }
        };

        if usage.is_threshold_reached(self.threshold) {
            warn!("running the scheduled thread cleanup");
            for worker in self.workers {
                worker.schedule_target_dir_cleanup();
            }
        }
    }
}
