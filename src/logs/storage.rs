use crate::config::Config;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct StoredRecord {
    level: Level,
    message: String,
}

struct InnerStorage {
    records: Vec<StoredRecord>,
    size: usize,
    truncated: bool,
}

#[derive(Clone)]
pub struct LogStorage {
    inner: Arc<Mutex<InnerStorage>>,
    min_level: LevelFilter,
    max_size: usize,
    max_lines: usize,
}

impl LogStorage {
    pub(crate) fn new(min_level: LevelFilter, config: &Config) -> Self {
        LogStorage {
            inner: Arc::new(Mutex::new(InnerStorage {
                records: Vec::new(),
                truncated: false,
                size: 0,
            })),
            min_level,
            max_size: config.sandbox.build_log_max_size.to_bytes(),
            max_lines: config.sandbox.build_log_max_lines,
        }
    }

    pub(crate) fn duplicate(&self) -> LogStorage {
        let inner = self.inner.lock().unwrap();
        LogStorage {
            inner: Arc::new(Mutex::new(InnerStorage {
                records: inner.records.clone(),
                truncated: inner.truncated,
                size: inner.size,
            })),
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
        if inner.records.len() >= self.max_lines {
            inner.records.push(StoredRecord {
                level: Level::Warn,
                message: "too many lines in the log, truncating it".into(),
            });
            inner.truncated = true;
            return;
        }
        let message = record.args().to_string();
        if inner.size + message.len() >= self.max_size {
            inner.records.push(StoredRecord {
                level: Level::Warn,
                message: "too much data in the log, truncating it".into(),
            });
            inner.truncated = true;
            return;
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

#[cfg(test)]
mod tests {
    use super::{LogStorage, StoredRecord};
    use crate::config::Config;
    use crate::logs;
    use crate::prelude::*;
    use crate::utils::size::Size;
    use log::{Level, LevelFilter};

    #[test]
    fn test_log_storage() {
        logs::init_test();
        let config = Config::default();

        let storage = LogStorage::new(LevelFilter::Info, &config);
        logs::capture(&storage, || {
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
        logs::init_test();

        let mut config = Config::default();
        config.sandbox.build_log_max_size = Size::Kilobytes(4);

        let storage = LogStorage::new(LevelFilter::Info, &config);
        logs::capture(&storage, || {
            let content = (0..Size::Kilobytes(8).to_bytes())
                .map(|_| '.')
                .collect::<String>();
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
        logs::init_test();

        let mut config = Config::default();
        config.sandbox.build_log_max_lines = 100;

        let storage = LogStorage::new(LevelFilter::Info, &config);
        logs::capture(&storage, || {
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
