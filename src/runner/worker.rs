use crate::crates::Crate;
use crate::experiments::{Experiment, Mode};
use crate::prelude::*;
use crate::results::{BrokenReason, TestResult, WriteResults};
use crate::runner::tasks::{Task, TaskStep};
use crate::runner::OverrideResult;
use crate::utils;
use rustwide::logging::LogStorage;
use rustwide::{BuildDirectory, Workspace};
use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::Duration;

pub(super) struct Worker<'a, DB: WriteResults + Sync> {
    name: String,
    workspace: &'a Workspace,
    build_dir: HashMap<&'a crate::toolchain::Toolchain, Mutex<BuildDirectory>>,
    ex: &'a Experiment,
    config: &'a crate::config::Config,
    db: &'a DB,
    target_dir_cleanup: AtomicBool,
    next_crate: &'a (dyn Fn() -> Fallible<Option<Crate>> + Send + Sync),
}

impl<'a, DB: WriteResults + Sync> Worker<'a, DB> {
    pub(super) fn new(
        name: String,
        workspace: &'a Workspace,
        ex: &'a Experiment,
        config: &'a crate::config::Config,
        db: &'a DB,
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
            db,
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
    ) -> Result<(), (failure::Error, TestResult)> {
        info!("running task: {:?}", task);

        let mut res = Ok(());
        let max_attempts = 5;
        for run in 1..=max_attempts {
            // If we're running a task, we call ourselves healthy.
            crate::agent::set_healthy();

            res = task.run(
                self.config,
                self.workspace,
                &self.build_dir,
                self.ex,
                self.db,
                storage,
            );

            // We retry task failing on the second toolchain (i.e., regressions). In
            // the future we might expand this list further but for now this helps
            // prevent spurious test failures and such.
            //
            // For now we make no distinction between build failures and test failures
            // here, but that may change if this proves too slow.
            let mut should_retry = false;
            if res.is_err() && self.ex.toolchains.len() == 2 {
                let toolchain = match &task.step {
                    TaskStep::Prepare => None,
                    TaskStep::BuildAndTest { tc, .. }
                    | TaskStep::BuildOnly { tc, .. }
                    | TaskStep::CheckOnly { tc, .. }
                    | TaskStep::Clippy { tc, .. }
                    | TaskStep::Rustdoc { tc, .. }
                    | TaskStep::UnstableFeatures { tc } => Some(tc),
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

            log::info!("Retrying task {:?} [{run}/{max_attempts}]", task);
        }
        if let Err(e) = res {
            error!("task {:?} failed", task);
            utils::report_failure(&e);

            let mut result = if self.config.is_broken(&task.krate) {
                TestResult::BrokenCrate(BrokenReason::Unknown)
            } else {
                TestResult::Error
            };

            for err in e.iter_chain() {
                if let Some(OverrideResult(res)) = err.downcast_ctx() {
                    result = res.clone();
                    break;
                }
            }

            return Err((e, result));
        }

        Ok(())
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
                    if let Err(e) = self.db.record_result(
                        self.ex,
                        tc,
                        &krate,
                        &LogStorage::from(self.config),
                        crate::results::EncodingType::Plain,
                        || {
                            warn!("crate skipped");
                            Ok(TestResult::Skipped)
                        },
                    ) {
                        crate::utils::report_failure(&e);
                    }
                }
                continue;
            }

            let logs = LogStorage::from(self.config);
            let prepare_task = Task {
                krate: krate.clone(),
                step: TaskStep::Prepare,
            };
            if let Err((err, test_result)) = &self.run_task(&prepare_task, &logs) {
                if let Err(e) =
                    prepare_task.mark_as_failed(self.ex, self.db, err, test_result, &logs)
                {
                    crate::utils::report_failure(&e);
                }
                for tc in &self.ex.toolchains {
                    if let Err(e) = self.db.record_result(
                        self.ex,
                        tc,
                        &krate,
                        &LogStorage::from(self.config),
                        crate::results::EncodingType::Plain,
                        || {
                            error!("this task or one of its parent failed!");
                            utils::report_failure(err);
                            Ok(test_result.clone())
                        },
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
                    },
                };

                // Fork logs off to distinct branch, so that each toolchain has its own log file,
                // while keeping the shared prepare step in common.
                let storage = logs.duplicate();
                if let Err((err, test_result)) = &self.run_task(&task, &storage) {
                    if let Err(e) =
                        task.mark_as_failed(self.ex, self.db, err, test_result, &storage)
                    {
                        crate::utils::report_failure(&e);
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

pub(super) struct DiskSpaceWatcher<'a, DB: WriteResults + Sync> {
    interval: Duration,
    threshold: f32,
    workers: &'a [Worker<'a, DB>],
    should_stop: Mutex<bool>,
    waiter: Condvar,
}

impl<'a, DB: WriteResults + Sync> DiskSpaceWatcher<'a, DB> {
    pub(super) fn new(interval: Duration, threshold: f32, workers: &'a [Worker<'a, DB>]) -> Self {
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
                warn!("Failed to check space remaining: {}", err);
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
