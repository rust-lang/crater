mod migrations;

use dirs::WORK_DIR;
use errors::*;
use r2d2::{CustomizeConnection, Pool};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, Row, Transaction};
use rusqlite::types::ToSql;
use std::sync::Arc;
use tempfile::NamedTempFile;

static DATABASE_PATH: &'static str = "server.db";

#[derive(Debug)]
struct ConnectionCustomizer;

impl CustomizeConnection<Connection, ::rusqlite::Error> for ConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> ::std::result::Result<(), ::rusqlite::Error> {
        conn.execute("PRAGMA foreign_keys = ON;", &[])?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Database {
    pool: Pool<SqliteConnectionManager>,
    // The tempfile is stored here to drop it after all the connections are closed
    tempfile: Option<Arc<NamedTempFile>>,
}

impl Database {
    pub fn open() -> Result<Self> {
        let path = WORK_DIR.join(DATABASE_PATH);
        Database::new(SqliteConnectionManager::file(path), None)
    }

    #[cfg(test)]
    pub fn temp() -> Result<Self> {
        let tempfile = NamedTempFile::new()?;
        Database::new(
            SqliteConnectionManager::file(tempfile.path()),
            Some(tempfile),
        )
    }

    fn new(conn: SqliteConnectionManager, tempfile: Option<NamedTempFile>) -> Result<Self> {
        let pool = Pool::builder()
            .connection_customizer(Box::new(ConnectionCustomizer))
            .build(conn)?;

        migrations::execute(&mut pool.get()? as &mut Connection)?;

        Ok(Database {
            pool,
            tempfile: tempfile.map(Arc::new),
        })
    }

    pub fn transaction<T, F: FnOnce(&TransactionHandle) -> Result<T>>(&self, f: F) -> Result<T> {
        let mut conn = self.pool.get()?;
        let handle = TransactionHandle {
            transaction: conn.transaction()?,
        };

        let result = f(&handle);
        handle.commit()?;
        result
    }
}

pub struct TransactionHandle<'a> {
    transaction: Transaction<'a>,
}

impl<'a> TransactionHandle<'a> {
    pub fn commit(self) -> Result<()> {
        self.transaction.commit()?;
        Ok(())
    }

    pub fn rollback(self) -> Result<()> {
        self.transaction.rollback()?;
        Ok(())
    }
}

pub trait QueryUtils {
    fn with_conn<T, F: FnOnce(&Connection) -> Result<T>>(&self, f: F) -> Result<T>;

    fn exists(&self, sql: &str, params: &[&ToSql]) -> Result<bool> {
        self.with_conn(|conn| {
            let mut prepared = conn.prepare(sql)?;
            Ok(prepared.exists(params)?)
        })
    }

    fn execute(&self, sql: &str, params: &[&ToSql]) -> Result<()> {
        self.with_conn(|conn| {
            let mut prepared = conn.prepare(sql)?;
            prepared.execute(params)?;
            Ok(())
        })
    }

    fn get_row<T, F: FnMut(&Row) -> T>(
        &self,
        sql: &str,
        params: &[&ToSql],
        func: F,
    ) -> Result<Option<T>> {
        self.with_conn(|conn| {
            let mut prepared = conn.prepare(sql)?;
            let mut iter = prepared.query_map(params, func)?;

            if let Some(item) = iter.next() {
                Ok(Some(item?))
            } else {
                Ok(None)
            }
        })
    }

    fn query<T, F: FnMut(&Row) -> T>(
        &self,
        sql: &str,
        params: &[&ToSql],
        func: F,
    ) -> Result<Vec<T>> {
        self.with_conn(|conn| {
            let mut prepared = conn.prepare(sql)?;
            let rows = prepared.query_map(params, func)?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            Ok(results)
        })
    }
}

impl QueryUtils for Database {
    fn with_conn<T, F: FnOnce(&Connection) -> Result<T>>(&self, f: F) -> Result<T> {
        f(&self.pool.get()? as &Connection)
    }
}

impl<'a> QueryUtils for TransactionHandle<'a> {
    fn with_conn<T, F: FnOnce(&Connection) -> Result<T>>(&self, f: F) -> Result<T> {
        f(&self.transaction)
    }
}
