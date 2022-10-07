mod tasks;
mod test;
mod unstable_features;
mod worker;

use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::{Experiment, Mode};
use crate::prelude::*;
use crate::results::{TestResult, WriteResults};
use crate::runner::worker::{DiskSpaceWatcher, Worker};
use crossbeam_utils::thread::{scope, ScopedJoinHandle};
use rustwide::logging::LogStorage;
use rustwide::Workspace;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const DISK_SPACE_WATCHER_INTERVAL: Duration = Duration::from_secs(30);
const DISK_SPACE_WATCHER_THRESHOLD: f32 = 0.80;

#[derive(Debug, Fail)]
#[fail(display = "overridden task result to {}", _0)]
pub struct OverrideResult(TestResult);

struct RunnerStateInner {
    prepare_logs: HashMap<Crate, LogStorage>,
}

struct RunnerState {
    inner: Mutex<RunnerStateInner>,
}

impl RunnerState {
    fn new() -> Self {
        RunnerState {
            inner: Mutex::new(RunnerStateInner {
                prepare_logs: HashMap::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<RunnerStateInner> {
        self.inner.lock().unwrap()
    }
}

pub fn run_ex<DB: WriteResults + Sync>(
    ex: &Experiment,
    workspace: &Workspace,
    db: &DB,
    threads_count: usize,
    config: &Config,
    next_crate: &(dyn Fn() -> Fallible<Option<Crate>> + Send + Sync),
) -> Fallible<()> {
    // Attempt to spin indefinitely until docker is up. Ideally, we would
    // decomission this agent until docker is up, instead of leaving the
    // assigned crates to 'hang' until we get our act together. In practice, we
    // expect workers to be around most of the time (just sometimes being
    // restarted etc.) and so the assigned crates shouldn't hang for long.
    //
    // If we return an Err(...) from this function, then currently that is
    // treated as a hard failure of the underlying experiment, but this error
    // has nothing to do with the experiment, so shouldn't be reported as such.
    //
    // In the future we'll want to *alert* on this error so that a human can
    // investigate, but the hope is that in practice docker is just being slow
    // or similar and this will fix itself, which currently makes the most sense
    // given low human resources. Additionally, it'll be indirectly alerted
    // through the worker being "down" according to our progress metrics, since
    // jobs won't be completed.
    let mut i = 0;
    while !rustwide::cmd::docker_running(workspace) {
        log::error!(
            "docker is not currently up, waiting for it to start (tried {} times)",
            i
        );
        i += 1;
    }

    crate::agent::set_healthy();

    info!("uninstalling toolchains...");
    // Clean out all the toolchains currently installed. This minimizes the
    // amount of disk space used by the base system, letting the task execution
    // proceed slightly faster than it would otherwise.
    for tc in workspace.installed_toolchains()? {
        // But don't uninstall it if we're going to reinstall in a couple lines.
        // And don't uninstall stable, since that is mainly used for
        // installing tools.
        if !tc.is_needed_by_rustwide() && !ex.toolchains.iter().any(|t| tc == t.source) {
            tc.uninstall(workspace)?;
        }
    }

    info!("preparing the execution...");
    for tc in &ex.toolchains {
        tc.install(workspace)?;
        if ex.mode == Mode::Clippy {
            tc.add_component(workspace, "clippy")?;
        }
        if let Some(requested_target) = &tc.target {
            tc.add_target(workspace, requested_target)?;
        }
    }

    info!("running tasks in {} threads...", threads_count);

    let state = RunnerState::new();

    let workers = (0..threads_count)
        .map(|i| {
            Worker::new(
                format!("worker-{}", i),
                workspace,
                ex,
                config,
                &state,
                db,
                next_crate,
            )
        })
        .collect::<Vec<_>>();

    let disk_watcher = DiskSpaceWatcher::new(
        DISK_SPACE_WATCHER_INTERVAL,
        DISK_SPACE_WATCHER_THRESHOLD,
        &workers,
    );

    let r = scope(|scope| -> Fallible<()> {
        let mut threads = Vec::new();

        for worker in &workers {
            let join =
                scope
                    .builder()
                    .name(worker.name().into())
                    .spawn(move |_| -> Fallible<()> {
                        match worker.run() {
                            Ok(()) => Ok(()),
                            Err(r) => {
                                log::warn!("worker {} failed: {:?}", worker.name(), r);
                                Err(r)
                            }
                        }
                    })?;
            threads.push(join);
        }
        let disk_watcher_thread =
            scope
                .builder()
                .name("disk-space-watcher".into())
                .spawn(|_| {
                    disk_watcher.run();
                    Ok(())
                })?;

        let clean_exit = join_threads(threads.into_iter());
        disk_watcher.stop();
        let disk_watcher_clean_exit = join_threads(std::iter::once(disk_watcher_thread));

        if clean_exit && disk_watcher_clean_exit {
            Ok(())
        } else {
            bail!("some threads returned an error");
        }
    });

    match r {
        Ok(r) => r,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn join_threads<'a, I>(iter: I) -> bool
where
    I: Iterator<Item = ScopedJoinHandle<'a, Fallible<()>>>,
{
    let mut clean_exit = true;
    for thread in iter {
        match thread.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                crate::utils::report_failure(&err);
                clean_exit = false;
            }
            Err(panic) => {
                crate::utils::report_panic(&panic);
                clean_exit = false;
            }
        }
    }
    clean_exit
}
