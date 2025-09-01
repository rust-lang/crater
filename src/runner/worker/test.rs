use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::time::Duration;

use crate::runner::DiskSpaceWatcher;

#[derive(Default, Debug)]
struct PurgeTracker {
    count: Mutex<usize>,
    wait: Condvar,
}

impl PurgeTracker {
    #[track_caller]
    fn assert_count_eq(&self, count: usize) {
        let guard = self.count.lock().unwrap();
        let (g, timer) = self
            .wait
            .wait_timeout_while(guard, Duration::from_secs(10), |g| *g != count)
            .unwrap();
        assert!(
            !timer.timed_out(),
            "timed out while waiting for {} to equal {}",
            *g,
            count
        );
        assert_eq!(*g, count);
    }
}

impl super::ToClean for PurgeTracker {
    fn purge(&self) {
        *self.count.lock().unwrap() += 1;
        self.wait.notify_all();
    }
}

#[test]
fn check_cleanup_single_worker() {
    let _ = env_logger::try_init();
    let tracker = Arc::new(PurgeTracker::default());
    let watcher = DiskSpaceWatcher::new(Duration::from_secs(60), 0.8, 1);
    let done = &AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            for _ in 0..3 {
                watcher.clean(&*tracker);
            }
            done.store(true, Ordering::Relaxed);
        });

        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                watcher.worker_idle(false);
            }
        });
    });

    tracker.assert_count_eq(3);
}

#[test]
fn check_cleanup_multi_worker() {
    let _ = env_logger::try_init();
    let tracker = Arc::new(PurgeTracker::default());
    let watcher = DiskSpaceWatcher::new(Duration::from_secs(60), 0.8, 3);
    let done = &AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            for _ in 0..5 {
                watcher.clean(&*tracker);
            }
            done.store(true, Ordering::Relaxed);
        });

        for _ in 0..3 {
            s.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    watcher.worker_idle(false);
                }
            });
        }
    });

    tracker.assert_count_eq(5);
}
