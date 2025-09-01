mod tasks;
mod test;
mod unstable_features;
mod worker;

use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::{Experiment, Mode};
use crate::prelude::*;
use crate::results::TestResult;
use crate::runner::worker::{DiskSpaceWatcher, Worker};
use rustwide::Workspace;
use std::sync::Arc;
use std::thread::scope;
use std::time::Duration;
pub use worker::RecordProgress;

const DISK_SPACE_WATCHER_INTERVAL: Duration = Duration::from_secs(30);
const DISK_SPACE_WATCHER_THRESHOLD: f32 = 0.80;

#[derive(Debug, thiserror::Error)]
#[error("overridden task result to {0}")]
pub struct OverrideResult(TestResult);

pub fn run_ex(
    ex: &Experiment,
    workspace: &Workspace,
    api: &dyn RecordProgress,
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
        log::error!("docker is not currently up, waiting for it to start (tried {i} times)");
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

    info!("running tasks in {threads_count} threads...");

    let workers = (0..threads_count)
        .map(|i| {
            Worker::new(
                format!("worker-{i}"),
                workspace,
                ex,
                config,
                api,
                next_crate,
            )
        })
        .collect::<Vec<_>>();

    let disk_watcher = DiskSpaceWatcher::new(
        DISK_SPACE_WATCHER_INTERVAL,
        DISK_SPACE_WATCHER_THRESHOLD,
        workers.len(),
    );

    for worker in workers.iter() {
        let disk_watcher = Arc::clone(&disk_watcher);
        assert!(worker
            .between_crates
            .set(Box::new(move |is_permanent| {
                disk_watcher.worker_idle(is_permanent);
            }))
            .is_ok());
    }

    scope(|scope1| {
        std::thread::Builder::new()
            .name("disk-space-watcher".into())
            .spawn_scoped(scope1, || {
                disk_watcher.run(workspace);
            })
            .unwrap();

        scope(|scope| {
            for worker in &workers {
                std::thread::Builder::new()
                    .name(worker.name().into())
                    .spawn_scoped(scope, move || -> Fallible<()> {
                        match worker.run() {
                            Ok(()) => Ok(()),
                            Err(r) => {
                                log::warn!("worker {} failed: {:?}", worker.name(), r);
                                Err(r)
                            }
                        }
                    })
                    .unwrap();
            }
        });

        disk_watcher.stop();
    });

    Ok(())
}
