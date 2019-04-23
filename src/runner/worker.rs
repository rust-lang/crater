use crate::config::Config;
use crate::docker::DockerEnv;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::{BrokenReason, TestResult, WriteResults};
use crate::runner::graph::{TasksGraph, WalkResult};
use crate::runner::{OverrideResult, RunnerState};
use crate::utils;
use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;

pub(super) struct Worker<'a, DB: WriteResults + Sync> {
    name: String,
    ex: &'a Experiment,
    config: &'a Config,
    graph: &'a Mutex<TasksGraph>,
    state: &'a RunnerState,
    db: &'a DB,
    docker_env: &'a DockerEnv,
    parked_threads: &'a Mutex<HashMap<thread::ThreadId, thread::Thread>>,
}

impl<'a, DB: WriteResults + Sync> Worker<'a, DB> {
    pub(super) fn new(
        name: String,
        ex: &'a Experiment,
        config: &'a Config,
        graph: &'a Mutex<TasksGraph>,
        state: &'a RunnerState,
        db: &'a DB,
        docker_env: &'a DockerEnv,
        parked_threads: &'a Mutex<HashMap<thread::ThreadId, thread::Thread>>,
    ) -> Self {
        Worker {
            name,
            ex,
            config,
            graph,
            state,
            db,
            docker_env,
            parked_threads,
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn run(&self) -> Fallible<()> {
        // This uses a `loop` instead of a `while let` to avoid locking the graph too much
        loop {
            let walk_result = self.graph.lock().unwrap().next_task(self.ex, self.db);
            match walk_result {
                WalkResult::Task(id, task) => {
                    info!("running task: {:?}", task);
                    if let Err(e) =
                        task.run(self.config, self.ex, self.db, self.docker_env, self.state)
                    {
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
}
