//! rustwide's logging system and related utilities.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::cell::RefCell;
use std::fmt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Once,
};
use std::thread::LocalKey;

static INIT_LOGS: Once = Once::new();
static INITIALIZED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SCOPED: RefCell<Vec<Box<dyn Log>>> = RefCell::new(Vec::new());
}

struct ScopedLogger {
    global: Option<Box<dyn Log>>,
    scoped: &'static LocalKey<RefCell<Vec<Box<dyn Log>>>>,
}

impl ScopedLogger {
    fn new(
        global: Option<Box<dyn Log>>,
        scoped: &'static LocalKey<RefCell<Vec<Box<dyn Log>>>>,
    ) -> Self {
        ScopedLogger { global, scoped }
    }

    fn each<F: FnMut(&dyn Log)>(&self, mut f: F) {
        if let Some(global) = &self.global {
            f(global.as_ref());
        }
        self.scoped.with(|scoped| {
            for logger in &*scoped.borrow() {
                f(logger.as_ref());
            }
        });
    }
}

impl Log for ScopedLogger {
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

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct StoredRecord {
    level: Level,
    message: String,
}

#[derive(Clone)]
struct InnerStorage {
    records: Vec<StoredRecord>,
    size: usize,
    truncated: bool,
}

/// Store logs captured by [`capture`] and retrieve them later.
///
/// The storage can have a maximum size and line limit, to prevent unbounded logging from exausting
/// system memory. It can be used from multiple threads at the same time. To output the stored log
/// entries you can call the `to_string()` method, which will return a string representation of
/// them.
///
/// [`capture`]: fn.capture.html
#[derive(Clone)]
pub struct LogStorage {
    inner: Arc<Mutex<InnerStorage>>,
    min_level: LevelFilter,
    max_size: Option<usize>,
    max_lines: Option<usize>,
}

impl LogStorage {
    /// Create a new log storage.
    pub fn new(min_level: LevelFilter) -> Self {
        LogStorage {
            inner: Arc::new(Mutex::new(InnerStorage {
                records: Vec::new(),
                truncated: false,
                size: 0,
            })),
            min_level,
            max_size: None,
            max_lines: None,
        }
    }

    /// Set the maximum amount of bytes stored in this struct before truncating the output.
    pub fn set_max_size(&mut self, size: usize) {
        self.max_size = Some(size);
    }

    /// Set the maximum amount of lines stored in this struct before truncating the output.
    pub fn set_max_lines(&mut self, lines: usize) {
        self.max_lines = Some(lines);
    }

    /// Duplicate the log storage, returning a new, unrelated storage with the same content and
    /// configuration.
    pub fn duplicate(&self) -> LogStorage {
        let inner = self.inner.lock().unwrap();
        LogStorage {
            inner: Arc::new(Mutex::new(inner.clone())),
            min_level: self.min_level,
            max_size: self.max_size,
            max_lines: self.max_lines,
        }
    }
}

impl Log for LogStorage {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() > self.min_level
    }

    fn log(&self, record: &Record) {
        if record.level() > self.min_level {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.truncated {
            return;
        }
        if let Some(max_lines) = self.max_lines {
            if inner.records.len() >= max_lines {
                inner.records.push(StoredRecord {
                    level: Level::Warn,
                    message: "too many lines in the log, truncating it".into(),
                });
                inner.truncated = true;
                return;
            }
        }
        let message = record.args().to_string();
        if let Some(max_size) = self.max_size {
            if inner.size + message.len() >= max_size {
                inner.records.push(StoredRecord {
                    level: Level::Warn,
                    message: "too much data in the log, truncating it".into(),
                });
                inner.truncated = true;
                return;
            }
        }
        inner.size += message.len();
        inner.records.push(StoredRecord {
            level: record.level(),
            message,
        });
    }

    fn flush(&self) {}
}

impl fmt::Display for LogStorage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let inner = self.inner.lock().unwrap();
        for record in &inner.records {
            writeln!(f, "[{}] {}", record.level, record.message)?;
        }
        Ok(())
    }
}

/// Capture all log messages emitted inside a closure.
///
/// This function will capture all the message the provided closure emitted **in the current
/// thread**, forwarding them to the provided [`LogStorage`]. rustwide's logging system needs to be
/// initialized before calling this function (either with [`init`] or [`init_with`]).
///
/// ## Example
///
/// ```
/// # rustwide::logging::init();
/// use log::{info, debug, LevelFilter};
/// use rustwide::logging::{self, LogStorage};
///
/// let storage = LogStorage::new(LevelFilter::Info);
/// logging::capture(&storage, || {
///     info!("foo");
///     debug!("bar");
/// });
///
/// assert_eq!("[INFO] foo\n", storage.to_string());
/// ```
///
/// [`LogStorage`]: struct.LogStorage.html
/// [`init`]: fn.init.html
/// [`init_with`]: fn.init_with.html
pub fn capture<R>(storage: &LogStorage, f: impl FnOnce() -> R) -> R {
    if !INITIALIZED.load(Ordering::SeqCst) {
        panic!("called capture without initializing rustwide::logging");
    }

    let storage = Box::new(storage.clone());
    SCOPED.with(|scoped| scoped.borrow_mut().push(storage));
    let result = f();
    SCOPED.with(|scoped| {
        let _ = scoped.borrow_mut().pop();
    });
    result
}

/// Initialize rustwide's logging system, enabling the use of the [`capture`] function.
///
/// This method will override any existing logger previously set and it will not show any log
/// message to the user. If you want to also add your own logger you should use the [`init_with`]
/// function.
///
/// [`capture`]: fn.capture.html
/// [`init_with`]: fn.init_with.html
pub fn init() {
    init_inner(None)
}

/// Initialize rustwide's logging system wrapping an existing logger, enabling the use of the
/// [`capture`] function.
///
/// If you don't want to add your own logger you should use the [`init`] function.
///
/// [`capture`]: fn.capture.html
/// [`init`]: fn.init.html
pub fn init_with<L: Log + 'static>(logger: L) {
    init_inner(Some(Box::new(logger)));
}

fn init_inner(logger: Option<Box<dyn Log>>) {
    INITIALIZED.store(true, Ordering::SeqCst);
    INIT_LOGS.call_once(|| {
        let multi = ScopedLogger::new(logger, &SCOPED);
        log::set_logger(Box::leak(Box::new(multi))).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
}

#[cfg(test)]
mod tests {
    use super::{LogStorage, StoredRecord};
    use crate::logging;
    use log::{info, trace, warn, Level, LevelFilter};

    #[test]
    fn test_log_storage() {
        logging::init();

        let storage = LogStorage::new(LevelFilter::Info);
        logging::capture(&storage, || {
            info!("an info record");
            warn!("a warn record");
            trace!("a trace record");
        });

        assert_eq!(
            storage.inner.lock().unwrap().records,
            vec![
                StoredRecord {
                    level: Level::Info,
                    message: "an info record".to_string(),
                },
                StoredRecord {
                    level: Level::Warn,
                    message: "a warn record".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_too_much_content() {
        logging::init();

        let mut storage = LogStorage::new(LevelFilter::Info);
        storage.set_max_size(1024);
        logging::capture(&storage, || {
            let content = (0..2048).map(|_| '.').collect::<String>();
            info!("{}", content);
        });

        let inner = storage.inner.lock().unwrap();
        assert_eq!(inner.records.len(), 1);
        assert!(inner
            .records
            .last()
            .unwrap()
            .message
            .contains("too much data"));
    }

    #[test]
    fn test_too_many_lines() {
        logging::init();

        let mut storage = LogStorage::new(LevelFilter::Info);
        storage.set_max_lines(100);
        logging::capture(&storage, || {
            for _ in 0..200 {
                info!("a line");
            }
        });

        let inner = storage.inner.lock().unwrap();
        assert_eq!(inner.records.len(), 101);
        assert!(inner
            .records
            .last()
            .unwrap()
            .message
            .contains("too many lines"));
    }
}
