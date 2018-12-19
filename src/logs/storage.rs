use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct StoredRecord {
    level: Level,
    message: String,
}

struct InnerStorage {
    records: Vec<StoredRecord>,
}

#[derive(Clone)]
pub struct LogStorage {
    inner: Arc<Mutex<InnerStorage>>,
    min_level: LevelFilter,
}

impl LogStorage {
    pub(crate) fn new(min_level: LevelFilter) -> Self {
        LogStorage {
            inner: Arc::new(Mutex::new(InnerStorage {
                records: Vec::new(),
            })),
            min_level,
        }
    }

    pub(crate) fn duplicate(&self) -> LogStorage {
        let inner = self.inner.lock().unwrap();
        LogStorage {
            inner: Arc::new(Mutex::new(InnerStorage {
                records: inner.records.clone(),
            })),
            min_level: self.min_level,
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
        inner.records.push(StoredRecord {
            level: record.level(),
            message: record.args().to_string(),
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
