use crate::config::Config;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::{BrokenReason, TestResult, WriteResults};
use crate::runner::graph::{TasksGraph, WalkResult};
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
    graph: &'a Mutex<TasksGraph>,
    state: &'a RunnerState,
    db: &'a DB,
    parked_threads: &'a Condvar,
    target_dir_cleanup: AtomicBool,
}

impl<'a, DB: WriteResults + Sync> Worker<'a, DB> {
    pub(super) fn new(
        name: String,
        workspace: &'a Workspace,
        ex: &'a Experiment,
        config: &'a Config,
        graph: &'a Mutex<TasksGraph>,
        state: &'a RunnerState,
        db: &'a DB,
        parked_threads: &'a Condvar,
    ) -> Self {
        Worker {
            build_dir: Mutex::new(workspace.build_dir(&name)),
            name,
            workspace,
            ex,
            config,
            graph,
            state,
            db,
            parked_threads,
            target_dir_cleanup: AtomicBool::new(false),
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn run(&self) -> Fallible<()> {
        // This uses a `loop` instead of a `while let` to avoid locking the graph too much
        let mut guard = self.graph.lock().unwrap();
        loop {
            self.maybe_cleanup_target_dir()?;
            let walk_result = guard.next_task(self.ex, self.db, &self.name);
            match walk_result {
                WalkResult::Task(id, task) => {
                    drop(guard);
                    info!("running task: {:?}", task);
                    let res = task.run(
                        self.config,
                        self.workspace,
                        &self.build_dir,
                        self.ex,
                        self.db,
                        self.state,
                    );
                    guard = self.graph.lock().unwrap();
                    // Regardless of how this ends, they should get woken up.
                    self.parked_threads.notify_all();
                    if let Err(e) = res {
                        error!("task failed, marking childs as failed too: {:?}", task);
                        utils::report_failure(&e);

                        let mut result = if self.config.is_broken(&task.krate) {
                            &TestResult::BrokenCrate(BrokenReason::Unknown)
                        } else {
                            &TestResult::Error
                        };

                        for err in e.iter_chain() {
                            if let Some(&OverrideResult(ref res)) = err.downcast_ctx() {
                                result = res;
                                break;
                            }
                        }

                        guard.mark_as_failed(
                            id,
                            self.ex,
                            self.db,
                            self.state,
                            self.config,
                            &e,
                            result,
                            &self.name,
                        )?;
                    } else {
                        guard.mark_as_completed(id);
                    }
                }
                WalkResult::Blocked => {
                    guard = self.parked_threads.wait(guard).unwrap();
                }
                WalkResult::NotBlocked => unreachable!("NotBlocked leaked from the run"),
                WalkResult::Finished => {
                    // A blocked thread may be waiting on the root node, in
                    // which case this is crucial to avoiding a deadlock.
                    self.parked_threads.notify_all();
                    drop(guard);
                    break;
                }
            }
        }

        Ok(())
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
    threshold: u32,
    workers: &'a [Worker<'a, DB>],
    should_stop: Mutex<bool>,
    waiter: Condvar,
}

impl<'a, DB: WriteResults + Sync> DiskSpaceWatcher<'a, DB> {
    pub(super) fn new(interval: Duration, threshold: u32, workers: &'a [Worker<'a, DB>]) -> Self {
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

        if !usage.has_gigabytes_left(self.threshold) {
            warn!("running the scheduled thread cleanup");
            for worker in self.workers {
                worker.schedule_target_dir_cleanup();
            }
        }
    }
}
