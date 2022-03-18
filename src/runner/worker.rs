use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::{Experiment, Mode};
use crate::prelude::*;
use crate::results::{BrokenReason, TestResult, WriteResults};
use crate::runner::tasks::{Task, TaskStep};
use crate::runner::{OverrideResult, RunnerState};
use crate::utils;
use rustwide::{BuildDirectory, Workspace};
use std::sync::Condvar;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::Duration;

pub(super) struct Worker<'a, DB: WriteResults + Sync> {
    name: String,
    workspace: &'a Workspace,
    build_dir: Mutex<BuildDirectory>,
    ex: &'a Experiment,
    config: &'a Config,
    crates: &'a Mutex<Vec<Crate>>,
    state: &'a RunnerState,
    db: &'a DB,
    target_dir_cleanup: AtomicBool,
}

impl<'a, DB: WriteResults + Sync> Worker<'a, DB> {
    pub(super) fn new(
        name: String,
        workspace: &'a Workspace,
        ex: &'a Experiment,
        config: &'a Config,
        crates: &'a Mutex<Vec<Crate>>,
        state: &'a RunnerState,
        db: &'a DB,
    ) -> Self {
        Worker {
            build_dir: Mutex::new(workspace.build_dir(&name)),
            name,
            workspace,
            ex,
            config,
            crates,
            state,
            db,
            target_dir_cleanup: AtomicBool::new(false),
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    fn run_task(&self, task: &Task) -> Result<(), (failure::Error, TestResult)> {
        info!("running task: {:?}", task);
        let res = task.run(
            self.config,
            self.workspace,
            &self.build_dir,
            self.ex,
            self.db,
            self.state,
        );
        if let Err(e) = res {
            error!("task {:?} failed", task);
            utils::report_failure(&e);

            let mut result = if self.config.is_broken(&task.krate) {
                TestResult::BrokenCrate(BrokenReason::Unknown)
            } else {
                TestResult::Error
            };

            for err in e.iter_chain() {
                if let Some(&OverrideResult(ref res)) = err.downcast_ctx() {
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
            let krate = if let Some(next) = self.crates.lock().unwrap().pop() {
                next
            } else {
                // We're done if no more crates left.
                return Ok(());
            };

            self.maybe_cleanup_target_dir()?;

            info!("{} processing crate {}", self.name, krate);

            let mut tasks = Vec::new();

            if !self.ex.ignore_blacklist && self.config.should_skip(&krate) {
                for tc in &self.ex.toolchains {
                    tasks.push(Task {
                        krate: krate.clone(),
                        step: TaskStep::Skip { tc: tc.clone() },
                    });
                }
            } else {
                tasks.push(Task {
                    krate: krate.clone(),
                    step: TaskStep::Prepare,
                });
                let quiet = self.config.is_quiet(&krate);
                for tc in &self.ex.toolchains {
                    tasks.push(Task {
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
                    });
                }
                tasks.push(Task {
                    krate: krate.clone(),
                    step: TaskStep::Cleanup,
                });
            }

            let mut result = Ok(());
            for task in tasks {
                if result.is_ok() {
                    result = self.run_task(&task);
                }
                if let Err((err, test_result)) = &result {
                    if let Err(e) = task.mark_as_failed(
                        self.ex,
                        self.db,
                        self.state,
                        self.config,
                        err,
                        test_result,
                    ) {
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
        self.build_dir.lock().unwrap().purge()?;
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
