use crate::config::Config;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::{BrokenReason, TestResult, WriteResults};
use crate::runner::graph::{TasksGraph, WalkResult};
use crate::runner::{OverrideResult, RunnerState};
use crate::utils;
use rustwide::{BuildDirectory, Workspace};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, RecvTimeoutError},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;
use systemstat::{Filesystem, Platform, System};

pub(super) struct Worker<'a, DB: WriteResults + Sync> {
    name: String,
    workspace: &'a Workspace,
    build_dir: Mutex<BuildDirectory>,
    ex: &'a Experiment,
    config: &'a Config,
    graph: &'a Mutex<TasksGraph>,
    state: &'a RunnerState,
    db: &'a DB,
    parked_threads: &'a Mutex<HashMap<thread::ThreadId, thread::Thread>>,
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
        parked_threads: &'a Mutex<HashMap<thread::ThreadId, thread::Thread>>,
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
        loop {
            self.maybe_cleanup_target_dir()?;
            let walk_result = self.graph.lock().unwrap().next_task(self.ex, self.db);
            match walk_result {
                WalkResult::Task(id, task) => {
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
                        error!("task failed, marking childs as failed too: {:?}", task);
                        utils::report_failure(&e);

                        let mut result = if self.config.is_broken(&task.krate) {
                            TestResult::BrokenCrate(BrokenReason::Unknown)
                        } else {
                            TestResult::Error
                        };

                        for err in e.iter_chain() {
                            if let Some(&OverrideResult(res)) = err.downcast_ctx() {
                                result = res;
                                break;
                            }
                        }

                        self.graph.lock().unwrap().mark_as_failed(
                            id,
                            self.ex,
                            self.db,
                            self.state,
                            self.config,
                            &e,
                            result,
                        )?;
                    } else {
                        self.graph.lock().unwrap().mark_as_completed(id);
                    }

                    // Unpark all the threads
                    let mut parked = self.parked_threads.lock().unwrap();
                    for (_id, thread) in parked.drain() {
                        thread.unpark();
                    }
                }
                WalkResult::Blocked => {
                    // Wait until another thread finished before looking for tasks again
                    // If the thread spuriously wake up (parking does not guarantee no
                    // spurious wakeups) it's not a big deal, it will just get parked again
                    {
                        let mut parked_threads = self.parked_threads.lock().unwrap();
                        let current = thread::current();
                        parked_threads.insert(current.id(), current);
                    }
                    thread::park();
                }
                WalkResult::NotBlocked => unreachable!("NotBlocked leaked from the run"),
                WalkResult::Finished => break,
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
    threshold: f32,
    workers: &'a [Worker<'a, DB>],
    stop_send: Arc<Mutex<mpsc::Sender<()>>>,
    stop_recv: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl<'a, DB: WriteResults + Sync> DiskSpaceWatcher<'a, DB> {
    pub(super) fn new(interval: Duration, threshold: f32, workers: &'a [Worker<'a, DB>]) -> Self {
        let (stop_send, stop_recv) = mpsc::channel();
        DiskSpaceWatcher {
            interval,
            threshold,
            workers,
            stop_send: Arc::new(Mutex::new(stop_send)),
            stop_recv: Arc::new(Mutex::new(stop_recv)),
        }
    }

    pub(super) fn stop(&self) {
        self.stop_send.lock().unwrap().send(()).unwrap();
    }

    pub(super) fn run(&self) -> Fallible<()> {
        loop {
            self.check()?;
            match self.stop_recv.lock().unwrap().recv_timeout(self.interval) {
                Ok(()) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => panic!("disconnected stop channel"),
            }
        }
    }

    fn check(&self) -> Fallible<()> {
        let fs = match self.current_mount() {
            Ok(fs) => fs,
            Err(e) => {
                // TODO: `current_mount` fails sometimes on Windows with ERROR_DEVICE_NOT_READY.
                warn!("Failed to check space remaining: {}", e);
                return Ok(());
            }
        };

        let usage = (fs.total.as_usize() - fs.free.as_usize()) as f32 / fs.total.as_usize() as f32;
        if usage < self.threshold {
            info!(
                "{} disk usage at {}%",
                fs.fs_mounted_on,
                (usage * 100.0) as u8
            );
        } else {
            warn!(
                "{} disk usage at {}%, which is over the threshold of {}%",
                fs.fs_mounted_on,
                (usage * 100.0) as u8,
                (self.threshold * 100.0) as u8,
            );

            for worker in self.workers {
                worker.schedule_target_dir_cleanup();
            }
            warn!("scheduled cleanup");
        }
        Ok(())
    }

    fn current_mount(&self) -> Fallible<Filesystem> {
        let current_dir = crate::dirs::WORK_DIR.canonicalize()?;
        let system = System::new();

        let mut found = None;
        let mut found_pos = std::usize::MAX;
        for mount in system.mounts()?.into_iter() {
            let path = Path::new(&mount.fs_mounted_on);
            for (i, ancestor) in current_dir.ancestors().enumerate() {
                if ancestor == path && i < found_pos {
                    found_pos = i;
                    found = Some(mount);
                    break;
                }
            }
        }
        found.ok_or_else(|| failure::err_msg("failed to find the current mount"))
    }
}
