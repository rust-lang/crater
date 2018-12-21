mod storage;

use log::{Log, Metadata, Record};
use std::cell::RefCell;
use std::sync::Once;
use std::thread::LocalKey;

pub use self::storage::LogStorage;

static INIT_LOGS: Once = Once::new();

thread_local! {
    static SCOPED: RefCell<Vec<Box<Log>>> = RefCell::new(Vec::new());
}

struct MultiLogger {
    global: Vec<Box<Log>>,
    scoped: &'static LocalKey<RefCell<Vec<Box<Log>>>>,
}

impl MultiLogger {
    fn new(global: Vec<Box<Log>>, scoped: &'static LocalKey<RefCell<Vec<Box<Log>>>>) -> Self {
        MultiLogger { global, scoped }
    }

    fn each<F: FnMut(&Log)>(&self, mut f: F) {
        for logger in &self.global {
            f(logger.as_ref());
        }
        self.scoped.with(|scoped| {
            for logger in &*scoped.borrow() {
                f(logger.as_ref());
            }
        });
    }
}

impl Log for MultiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let mut result = false;
        self.each(|logger| {
            if logger.enabled(metadata) {
                result = true;
            }
        });
        result
    }

    fn log(&self, record: &Record) {
        self.each(|logger| {
            logger.log(record);
        });
    }

    fn flush(&self) {
        self.each(|logger| {
            logger.flush();
        });
    }
}

pub fn capture<F, R, L>(storage: &L, f: F) -> R
where
    F: FnOnce() -> R,
    L: Log + Clone + 'static,
{
    let storage = Box::new(storage.clone());
    SCOPED.with(|scoped| scoped.borrow_mut().push(storage));
    let result = f();
    SCOPED.with(|scoped| {
        let _ = scoped.borrow_mut().pop();
    });
    result
}

pub fn init() {
    INIT_LOGS.call_once(|| {
        // Initialize env_logger
        // This doesn't use from_default_env() because it doesn't allow to override filter_module()
        // with the RUST_LOG environment variable
        let mut env = env_logger::Builder::new();
        env.filter_module("crater", log::LevelFilter::Info);
        if let Ok(content) = std::env::var("RUST_LOG") {
            env.parse(&content);
        }

        let multi = MultiLogger::new(vec![Box::new(env.build())], &SCOPED);
        log::set_boxed_logger(Box::new(multi)).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
}

#[cfg(test)]
pub(crate) fn init_test() {
    INIT_LOGS.call_once(|| {
        // Avoid setting up ENV_LOGGER inside tests
        let multi = MultiLogger::new(vec![], &SCOPED);
        log::set_boxed_logger(Box::new(multi)).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    })
}
