mod migrations;

use crate::dirs::WORK_DIR;
use crate::prelude::*;
use r2d2::{CustomizeConnection, Pool};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::types::ToSql;
use rusqlite::{Connection, Row, Transaction};
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

static LEGACY_DATABASE_PATHS: &[&str] = &["server.db"];
static DATABASE_PATH: &str = "crater.db";

#[derive(Debug)]
struct ConnectionCustomizer;

impl CustomizeConnection<Connection, ::rusqlite::Error> for ConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), ::rusqlite::Error> {
        conn.execute("PRAGMA foreign_keys = ON;", [])?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Database {
    pool: Pool<SqliteConnectionManager>,
    // The tempfile is stored here to drop it after all the connections are closed
    _tempfile: Option<Arc<NamedTempFile>>,
}

impl Database {
    pub fn open() -> Fallible<Self> {
        let path = WORK_DIR.join(DATABASE_PATH);
        if !path.exists() {
            // If the database doesn't exist check if it's present in a legacy path
            for legacy in LEGACY_DATABASE_PATHS {
                let legacy = WORK_DIR.join(legacy);
                if legacy.exists() {
                    // Rename the legacy database so it's present in the new path
                    ::std::fs::rename(&legacy, &path)?;
                    info!(
                        "Moved legacy database from {} to {}",
                        legacy.to_string_lossy(),
                        path.to_string_lossy()
                    );
                    break;
                }
            }
        }

        let path = WORK_DIR.join(DATABASE_PATH);
        std::fs::create_dir_all(&*WORK_DIR)?;
        Database::new(SqliteConnectionManager::file(path), None)
    }

    #[cfg(test)]
    pub fn temp() -> Fallible<Self> {
        let tempfile = NamedTempFile::new()?;
        Database::new(
            SqliteConnectionManager::file(tempfile.path()),
            Some(tempfile),
        )
    }

    fn new(conn: SqliteConnectionManager, tempfile: Option<NamedTempFile>) -> Fallible<Self> {
        let pool = Pool::builder()
            .connection_customizer(Box::new(ConnectionCustomizer))
            .build(conn)?;

        migrations::execute(&mut pool.get()? as &mut Connection)?;

        Ok(Database {
            pool,
            _tempfile: tempfile.map(Arc::new),
        })
    }

    pub fn transaction<T, F: FnOnce(&TransactionHandle) -> Fallible<T>>(
        &self,
        f: F,
    ) -> Fallible<T> {
        let mut conn = self.pool.get()?;
        let handle = TransactionHandle {
            transaction: conn.transaction()?,
        };

        match f(&handle) {
            Ok(res) => {
                handle.commit()?;
                Ok(res)
            }
            Err(err) => {
                handle.rollback()?;
                Err(err)
            }
        }
    }
}

pub struct TransactionHandle<'a> {
    transaction: Transaction<'a>,
}

impl<'a> TransactionHandle<'a> {
    pub fn commit(self) -> Fallible<()> {
        self.transaction.commit()?;
        Ok(())
    }

    pub fn rollback(self) -> Fallible<()> {
        self.transaction.rollback()?;
        Ok(())
    }
}

pub trait QueryUtils {
    fn with_conn<T, F: FnOnce(&Connection) -> Fallible<T>>(&self, f: F) -> Fallible<T>;

    fn exists(&self, sql: &str, params: &[&dyn ToSql]) -> Fallible<bool> {
        self.with_conn(|conn| {
            self.trace(sql, || {
                let mut prepared = conn.prepare(sql)?;
                Ok(prepared.exists(params)?)
            })
        })
    }

    fn execute(&self, sql: &str, params: &[&dyn ToSql]) -> Fallible<usize> {
        self.with_conn(|conn| {
            self.trace(sql, || {
                let mut prepared = conn.prepare_cached(sql)?;
                let changes = prepared.execute(params)?;
                Ok(changes)
            })
        })
    }

    fn execute_cached(&self, sql: &str, params: &[&dyn ToSql]) -> Fallible<usize> {
        self.with_conn(|conn| {
            self.trace(sql, || {
                let mut prepared = conn.prepare_cached(sql)?;
                let changes = prepared.execute(params)?;
                Ok(changes)
            })
        })
    }

    fn get_row<T, P>(
        &self,
        sql: &str,
        params: P,
        func: impl FnMut(&Row) -> rusqlite::Result<T>,
    ) -> Fallible<Option<T>>
    where
        P: rusqlite::Params,
    {
        self.with_conn(|conn| {
            self.trace(sql, || {
                let mut prepared = conn.prepare(sql)?;
                let mut iter = prepared.query_map(params, func)?;

                if let Some(item) = iter.next() {
                    Ok(Some(item?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    fn query<T, F: FnMut(&Row) -> rusqlite::Result<T>>(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
        func: F,
    ) -> Fallible<Vec<T>> {
        self.with_conn(|conn| {
            self.trace(sql, || {
                let mut prepared = conn.prepare(sql)?;
                let rows = prepared.query_map(params, func)?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row?);
                }

                Ok(results)
            })
        })
    }

    fn query_row<T, F: FnOnce(&Row) -> Fallible<T>>(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
        func: F,
    ) -> Fallible<Option<T>> {
        self.with_conn(|conn| {
            self.trace(sql, || {
                let mut prepared = conn.prepare(sql)?;
                let mut rows = prepared.query(params)?;
                if let Ok(Some(row)) = rows.next() {
                    return Ok(Some(func(row)?));
                }
                Ok(None)
            })
        })
    }

    fn trace<T, F: FnOnce() -> T>(&self, sql: &str, f: F) -> T {
        let start = Instant::now();
        let res = f();
        let elapsed = start.elapsed();
        // Log all queries that take at least 1/2 a second to execute.
        if elapsed.as_millis() > 500 {
            debug!("sql query \"{}\" executed in {:?}", sql, elapsed);
        } else {
            trace!("sql query \"{}\" executed in {:?}", sql, elapsed);
        }
        res
    }
}

impl QueryUtils for Database {
    fn with_conn<T, F: FnOnce(&Connection) -> Fallible<T>>(&self, f: F) -> Fallible<T> {
        f(&self.pool.get()? as &Connection)
    }
}

impl<'a> QueryUtils for TransactionHandle<'a> {
    fn with_conn<T, F: FnOnce(&Connection) -> Fallible<T>>(&self, f: F) -> Fallible<T> {
        f(&self.transaction)
    }
}
