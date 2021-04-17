mod graph;
mod tasks;
mod test;
mod unstable_features;
mod worker;

use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::{Experiment, Mode};
use crate::prelude::*;
use crate::results::{TestResult, WriteResults};
use crate::runner::graph::build_graph;
use crate::runner::worker::{DiskSpaceWatcher, Worker};
use crossbeam_utils::thread::{scope, ScopedJoinHandle};
use rustwide::logging::LogStorage;
use rustwide::Workspace;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const DISK_SPACE_WATCHER_INTERVAL: Duration = Duration::from_secs(300);
const DISK_SPACE_WATCHER_THRESHOLD: f32 = 0.85;

#[derive(Debug, Fail)]
#[fail(display = "overridden task result to {}", _0)]
pub struct OverrideResult(TestResult);

struct RunnerStateInner {
    prepare_logs: HashMap<Crate, LogStorage>,
}

struct RunnerState {
    inner: Mutex<RunnerStateInner>,

    // Jobserver ends for all workerse used by this runner. Don't need to be
    // protected by the mutex.
    pub read: File,
    pub write: File,
    pub path: std::mem::ManuallyDrop<tempfile::TempDir>,
}

impl RunnerState {
    fn new(cpus: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            "/usr/local/bin/jobserver-crater-fwd",
            dir.path().join("jobserver-crater-fwd"),
        )
        .unwrap();
        let file = dir.path().join("fifo");
        let fifo = std::ffi::CString::new(file.to_str().unwrap()).unwrap();
        unsafe {
            if libc::mkfifo(fifo.as_ptr(), 0o777) < 0 {
                panic!("failed to make to fifo");
            }
        }

        // We need threads here as otherwise opening the read end or the write
        // end will block until the other end is opened.
        let path = file.clone();
        let read_end =
            std::thread::spawn(move || OpenOptions::new().read(true).open(path).unwrap());
        let path = file;
        let write_end =
            std::thread::spawn(move || OpenOptions::new().write(true).open(path).unwrap());

        let mut write = write_end.join().unwrap();

        // Fill up the 'jobserver' with N tokens for each cpu we have.
        for _ in 0..cpus {
            assert!(write.write(&[b'|']).unwrap() == 1);
        }

        let read = read_end.join().unwrap();

        RunnerState {
            inner: Mutex::new(RunnerStateInner {
                prepare_logs: HashMap::new(),
            }),
            read,
            write,
            path: std::mem::ManuallyDrop::new(dir),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<RunnerStateInner> {
        self.inner.lock().unwrap()
    }
}

pub fn run_ex<DB: WriteResults + Sync>(
    ex: &Experiment,
    workspace: &Workspace,
    crates: &[Crate],
    db: &DB,
    threads_count: usize,
    config: &Config,
) -> Fallible<()> {
    if !rustwide::cmd::docker_running(workspace) {
        return Err(err_msg("docker is not running"));
    }

    info!("computing the tasks graph...");
    let graph = Mutex::new(build_graph(ex, crates, config));
    let parked_threads = Condvar::new();

    info!("uninstalling toolchains...");
    // Clean out all the toolchains currently installed. This minimizes the
    // amount of disk space used by the base system, letting the task execution
    // proceed slightly faster than it would otherwise.
    for tc in workspace.installed_toolchains()? {
        // But don't uninstall it if we're going to reinstall in a couple lines.
        if !ex.toolchains.iter().any(|t| tc == t.source) {
            tc.uninstall(workspace)?;
        }
    }

    info!("preparing the execution...");
    for tc in &ex.toolchains {
        tc.install(workspace)?;
        if ex.mode == Mode::Clippy {
            tc.add_component(workspace, "clippy")?;
        }
    }

    info!("running tasks in {} threads...", threads_count);

    let state = RunnerState::new(num_cpus::get());

    let workers = (0..threads_count)
        .map(|i| {
            Worker::new(
                format!("worker-{}", i),
                workspace,
                ex,
                config,
                &graph,
                &state,
                db,
                &parked_threads,
            )
        })
        .collect::<Vec<_>>();

    let disk_watcher = DiskSpaceWatcher::new(
        DISK_SPACE_WATCHER_INTERVAL,
        DISK_SPACE_WATCHER_THRESHOLD,
        &workers,
    );

    scope(|scope| -> Fallible<()> {
        let mut threads = Vec::new();

        for worker in &workers {
            let join =
                scope
                    .builder()
                    .name(worker.name().into())
                    .spawn(move || match worker.run() {
                        Ok(()) => Ok(()),
                        Err(r) => {
                            log::warn!("worker {} failed: {:?}", worker.name(), r);
                            Err(r)
                        }
                    })?;
            threads.push(join);
        }
        let disk_watcher_thread =
            scope
                .builder()
                .name("disk-space-watcher".into())
                .spawn(|| {
                    disk_watcher.run();
                    Ok(())
                })?;

        let clean_exit = join_threads(threads.drain(..));
        disk_watcher.stop();
        let disk_watcher_clean_exit = join_threads(std::iter::once(disk_watcher_thread));

        if clean_exit && disk_watcher_clean_exit {
            Ok(())
        } else {
            bail!("some threads returned an error");
        }
    })?;

    // Only the root node must be present
    let mut g = graph.lock().unwrap();
    assert!(g.next_task(ex, db, "master").is_finished());
    assert_eq!(g.pending_crates_count(), 0);

    Ok(())
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

pub fn dump_dot(ex: &Experiment, crates: &[Crate], config: &Config, dest: &Path) -> Fallible<()> {
    info!("computing the tasks graph...");
    let graph = build_graph(&ex, crates, config);

    info!("dumping the tasks graph...");
    ::std::fs::write(dest, format!("{:?}", graph.generate_dot()).as_bytes())?;

    info!("tasks graph available in {}", dest.to_string_lossy());

    Ok(())
}
