mod graph;
mod prepare;
mod tasks;
mod test;
mod toml_frobber;
mod unstable_features;

use config::Config;
use crossbeam_utils::thread::scope;
use docker::DockerEnv;
use experiments::Experiment;
use prelude::*;
use results::{FailureReason, TestResult, WriteResults};
use runner::graph::{build_graph, WalkResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use utils;

#[derive(Debug, Fail)]
#[fail(display = "overridden task result to {}", _0)]
pub struct OverrideResult(TestResult);

pub fn run_ex<DB: WriteResults + Sync>(
    ex: &Experiment,
    db: &DB,
    threads_count: usize,
    docker_env: &str,
    config: &Config,
) -> Fallible<()> {
    if !::docker::is_running() {
        return Err(err_msg("docker is not running"));
    }

    let res = run_ex_inner(ex, db, threads_count, docker_env, config);

    // Remove all the target dirs even if the experiment failed
    let target_dir = &::toolchain::ex_target_dir(&ex.name);
    if target_dir.exists() {
        utils::fs::remove_dir_all(target_dir)?;
    }

    res
}

fn run_ex_inner<DB: WriteResults + Sync>(
    ex: &Experiment,
    db: &DB,
    threads_count: usize,
    docker_env: &str,
    config: &Config,
) -> Fallible<()> {
    info!("ensuring all the tools are installed");
    ::tools::install()?;

    info!("loading docker environment...");
    let docker_env = DockerEnv::new(docker_env)?;

    info!("computing the tasks graph...");
    let graph = Mutex::new(build_graph(ex, config));

    info!("preparing the execution...");
    for tc in &ex.toolchains {
        tc.prepare()?;
    }

    info!("running tasks in {} threads...", threads_count);

    // An HashMap is used instead of an HashSet because Thread is not Eq+Hash
    let parked_threads: Mutex<HashMap<thread::ThreadId, thread::Thread>> =
        Mutex::new(HashMap::new());

    scope(|scope| -> Fallible<()> {
        let mut threads = Vec::new();

        for i in 0..threads_count {
            let name = format!("worker-{}", i);
            let join = scope.builder().name(name).spawn(|| -> Fallible<()> {
                // This uses a `loop` instead of a `while let` to avoid locking the graph too much
                loop {
                    let walk_result = graph.lock().unwrap().next_task(ex, db);
                    match walk_result {
                        WalkResult::Task(id, task) => {
                            info!("running task: {:?}", task);
                            if let Err(e) = task.run(config, ex, db, &docker_env) {
                                error!("task failed, marking childs as failed too: {:?}", task);
                                utils::report_failure(&e);

                                let mut result = if config.is_broken(&task.krate) {
                                    TestResult::BuildFail(FailureReason::Broken)
                                } else {
                                    TestResult::Error
                                };

                                for err in e.iter_chain() {
                                    if let Some(&OverrideResult(res)) = err.downcast_ctx() {
                                        result = res;
                                        break;
                                    }
                                }

                                graph
                                    .lock()
                                    .unwrap()
                                    .mark_as_failed(id, ex, db, &e, result)?;
                            } else {
                                graph.lock().unwrap().mark_as_completed(id);
                            }

                            // Unpark all the threads
                            let mut parked = parked_threads.lock().unwrap();
                            for (_id, thread) in parked.drain() {
                                thread.unpark();
                            }
                        }
                        WalkResult::Blocked => {
                            // Wait until another thread finished before looking for tasks again
                            // If the thread spuriously wake up (parking does not guarantee no
                            // spurious wakeups) it's not a big deal, it will just get parked again
                            {
                                let mut parked_threads = parked_threads.lock().unwrap();
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
            })?;
            threads.push(join);
        }

        let mut clean_exit = true;
        for thread in threads.drain(..) {
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    ::utils::report_failure(&err);
                    clean_exit = false;
                }
                Err(panic) => {
                    ::utils::report_panic(&panic);
                    clean_exit = false;
                }
            }
        }

        if clean_exit {
            Ok(())
        } else {
            bail!("some threads returned an error");
        }
    })?;

    // Only the root node must be present
    let mut g = graph.lock().unwrap();
    assert!(g.next_task(ex, db).is_finished());
    assert_eq!(g.pending_crates_count(), 0);

    Ok(())
}

pub fn dump_dot(ex: &Experiment, config: &Config, dest: &Path) -> Fallible<()> {
    info!("computing the tasks graph...");
    let graph = build_graph(&ex, config);

    info!("dumping the tasks graph...");
    ::std::fs::write(dest, format!("{:?}", graph.generate_dot()).as_bytes())?;

    info!("tasks graph available in {}", dest.to_string_lossy());

    Ok(())
}
