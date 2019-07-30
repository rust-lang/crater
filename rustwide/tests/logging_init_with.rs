use log::{info, LevelFilter, Log, Metadata, Record};
use rustwide::logging::{self, LogStorage};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone)]
struct DummyLogger {
    called: Arc<AtomicBool>,
}

impl DummyLogger {
    fn new() -> Self {
        DummyLogger {
            called: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Log for DummyLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, _record: &Record) {
        self.called.store(true, Ordering::SeqCst);
    }

    fn flush(&self) {}
}

#[test]
fn test_init_with() {
    let logger = DummyLogger::new();
    logging::init_with(logger.clone());

    let storage = LogStorage::new(LevelFilter::Info, 1024, 10);
    logging::capture(&storage, || {
        info!("Hello world!");
    });

    assert_eq!("[INFO] Hello world!\n", storage.to_string());
    assert!(logger.called.load(Ordering::SeqCst));
}
