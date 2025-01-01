use crate::crates::Crate;
use crate::db::{Database, QueryUtils};
use crate::experiments::{Experiment, Status};
use crate::prelude::*;
use crate::results::{
    DeleteResults, EncodedLog, EncodingType, ReadResults, TestResult, WriteResults,
};
use crate::toolchain::Toolchain;
use base64::Engine;
use rustwide::logging::{self, LogStorage};

#[derive(Deserialize)]
pub struct TaskResult {
    #[serde(rename = "crate")]
    pub krate: Crate,
    pub toolchain: Toolchain,
    pub result: TestResult,
    pub log: String,
}

#[derive(Deserialize)]
pub struct ProgressData {
    pub result: TaskResult,
    pub version: Option<(Crate, Crate)>,
}

pub struct DatabaseDB<'a> {
    db: &'a Database,
}

impl<'a> DatabaseDB<'a> {
    pub fn new(db: &'a Database) -> Self {
        DatabaseDB { db }
    }

    pub fn clear_stale_records(&self) -> Fallible<()> {
        // We limit ourselves to a small number of records at a time. This means this query
        // needs to run tends of thousands of times to purge records from a
        // single crater run, but it also means that each individual execution
        // is quite fast. That lets us run it often, without blocking other I/O
        // on the database for long.
        //
        // Currently this is run as we add new results into the database, which
        // does add some overhead to progress collection, but also gives us a
        // natural point to run this housekeeping.
        //
        // In practice so long as the limit here is >1 that also means we're
        // definitely going to keep up and not have more than (approximately)
        // one crater run in storage at any time, as old ones get purged
        // quite quickly after they finish.
        //
        // We also only purge from results here rather than cleaning up other
        // tables as this is just simpler and the results dominate the storage
        // size anyway. In the future we might expand this to other tables, but
        // for now that wouldn't really add enough value to be worth it.
        //
        // The query here would be simpler if rusqlite came with delete .. limit
        // support compiled in, but that's not likely to happen (see
        // https://github.com/rusqlite/rusqlite/issues/1111).
        self.db.execute(
            "delete from results where rowid in (
                select rowid from results where \
                    experiment in (select name from experiments where status = 'completed') \
                    limit 100
                )",
            &[],
        )?;
        self.db.execute(
            "delete from experiment_crates where rowid in (
                select rowid from experiment_crates where \
                    experiment in (select name from experiments where status = 'completed') \
                    limit 100
                )",
            &[],
        )?;

        Ok(())
    }

    pub fn store(
        &self,
        ex: &Experiment,
        data: &ProgressData,
        encoding_type: EncodingType,
    ) -> Fallible<()> {
        let krate = if let Some((old, new)) = &data.version {
            // If we're updating the name of the crate (typically changing the hash we found on
            // github) then we ought to also use that new name for marking the crate as complete.
            // Otherwise, we leave behind the old (unversioned) name and end up running this crate
            // many times, effectively never actually completing it.
            self.update_crate_version(ex, old, new)?;

            // sanity check that the previous name of the crate is the one we intended to run.
            if old.id() != data.result.krate.id() {
                log::warn!(
                    "Storing result under {} despite job intended for {} (with wrong name old={})",
                    new.id(),
                    data.result.krate.id(),
                    old.id(),
                );
            }

            new
        } else {
            &data.result.krate
        };

        self.store_result(
            ex,
            krate,
            &data.result.toolchain,
            &data.result.result,
            &base64::engine::general_purpose::STANDARD
                .decode(&data.result.log)
                .with_context(|| "invalid base64 log provided")?,
            encoding_type,
        )?;

        self.mark_crate_as_completed(ex, krate)?;

        Ok(())
    }

    fn mark_crate_as_completed(&self, ex: &Experiment, krate: &Crate) -> Fallible<usize> {
        self.db.execute(
            "UPDATE experiment_crates SET status = ?1 WHERE experiment = ?2 AND crate = ?3 \
             AND ( (SELECT COUNT(*) FROM results WHERE experiment = ?2 AND crate = ?3) > 1 )",
            &[&Status::Completed.to_string(), &ex.name, &krate.id()],
        )
    }

    fn store_result(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        res: &TestResult,
        log: &[u8],
        desired_encoding_type: EncodingType,
    ) -> Fallible<()> {
        let encoded_log = EncodedLog::from_plain_slice(log, desired_encoding_type)?;
        self.insert_into_results(ex, krate, toolchain, res, encoded_log)?;
        Ok(())
    }

    fn insert_into_results(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        res: &TestResult,
        log: EncodedLog,
    ) -> Fallible<usize> {
        log::info!(
            "insert {krate} for ex={ex:?} with tc={toolchain}; result={res:?}",
            krate = krate.id(),
            ex = &ex.name
        );
        self.db.execute(
            "INSERT INTO results (experiment, crate, toolchain, result, log, encoding) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            &[
                &ex.name,
                &krate.id(),
                &toolchain.to_string(),
                &res.to_string(),
                &log.as_slice(),
                &log.get_encoding_type().to_str(),
            ],
        )
    }
}

impl ReadResults for DatabaseDB<'_> {
    fn load_log(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<EncodedLog>> {
        self.db.get_row(
            "SELECT log, encoding FROM results \
             WHERE experiment = ?1 AND toolchain = ?2 AND crate = ?3 \
             LIMIT 1;",
            [&ex.name, &toolchain.to_string(), &krate.id()],
            |row| {
                let log: Vec<u8> = row.get("log")?;
                let encoding: String = row.get("encoding")?;
                let encoding = encoding.parse().unwrap();

                Ok(match encoding {
                    EncodingType::Plain => EncodedLog::Plain(log),
                    EncodingType::Gzip => EncodedLog::Gzip(log),
                })
            },
        )
    }

    fn load_test_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>> {
        Ok(self.db.query_row(
            "SELECT result FROM results \
                 WHERE experiment = ?1 AND toolchain = ?2 AND crate = ?3 \
                 LIMIT 1;",
            [&ex.name, &toolchain.to_string(), &krate.id()],
            |row| Ok(row.get_ref("result")?.as_str()?.parse::<TestResult>()?),
        )?)
    }
}

impl WriteResults for DatabaseDB<'_> {
    fn get_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>> {
        self.load_test_result(ex, toolchain, krate)
    }

    fn update_crate_version(&self, ex: &Experiment, old: &Crate, new: &Crate) -> Fallible<()> {
        self.db.execute(
            "UPDATE experiment_crates SET crate = ?1 WHERE experiment = ?2 AND crate = ?3;",
            &[&new.id(), &ex.name, &old.id()],
        )?;
        Ok(())
    }

    fn record_result<F>(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        storage: &LogStorage,
        encoding_type: EncodingType,
        f: F,
    ) -> Fallible<TestResult>
    where
        F: FnOnce() -> Fallible<TestResult>,
    {
        let result = logging::capture(storage, f)?;
        let output = storage.to_string();
        self.store_result(
            ex,
            krate,
            toolchain,
            &result,
            output.as_bytes(),
            encoding_type,
        )?;
        Ok(result)
    }
}

impl crate::runner::RecordProgress for DatabaseDB<'_> {
    fn record_progress(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        log: &[u8],
        result: &TestResult,
        version: Option<(&Crate, &Crate)>,
    ) -> Fallible<()> {
        self.store_result(ex, krate, toolchain, result, log, EncodingType::Plain)?;
        if let Some((old, new)) = version {
            self.update_crate_version(ex, old, new)?;
        }
        Ok(())
    }
}

impl DeleteResults for DatabaseDB<'_> {
    fn delete_all_results(&self, ex: &Experiment) -> Fallible<()> {
        self.db
            .execute("DELETE FROM results WHERE experiment = ?1;", &[&ex.name])?;
        Ok(())
    }

    fn delete_result(&self, ex: &Experiment, tc: &Toolchain, krate: &Crate) -> Fallible<()> {
        self.db.execute(
            "DELETE FROM results WHERE experiment = ?1 AND toolchain = ?2 AND crate = ?3;",
            &[&ex.name, &tc.to_string(), &krate.id()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use rustwide::logging::LogStorage;

    use super::{DatabaseDB, ProgressData, TaskResult};
    use crate::actions::{Action, ActionsCtx, CreateExperiment};
    use crate::config::Config;
    use crate::crates::{Crate, RegistryCrate};
    use crate::db::Database;
    use crate::experiments::Experiment;
    use crate::prelude::*;
    use crate::results::{
        DeleteResults, EncodedLog, EncodingType, FailureReason, ReadResults, TestResult,
        WriteResults,
    };
    use crate::toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    use std::collections::BTreeSet;

    #[test]
    fn test_versions() {
        let db = Database::temp().unwrap();
        let results = DatabaseDB::new(&db);
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment to attach the results to
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();
        let ex = Experiment::get(&db, "dummy").unwrap().unwrap();

        let crates = ex
            .get_crates(&db)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let build_fail = Crate::Local("build-fail".to_string());
        let updated = Crate::Local("updated".to_string());

        assert!(crates.contains(&build_fail));

        // update crate version
        results
            .update_crate_version(&ex, &build_fail, &updated)
            .unwrap();

        let crates = ex
            .get_crates(&db)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert!(!crates.contains(&build_fail));
        assert!(crates.contains(&updated));

        let updated_again = Crate::Local("updated, again".to_string());

        results
            .update_crate_version(&ex, &updated, &updated_again)
            .unwrap();

        let crates = ex
            .get_crates(&db)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert!(!crates.contains(&build_fail));
        assert!(!crates.contains(&updated));
        assert!(crates.contains(&updated_again));
    }

    #[test]
    fn test_results() {
        rustwide::logging::init();

        let db = Database::temp().unwrap();
        let results = DatabaseDB::new(&db);
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment to attach the results to
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();
        let ex = Experiment::get(&db, "dummy").unwrap().unwrap();

        let krate = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1".into(),
        });

        // Record a result with a message in it
        results
            .record_result(
                &ex,
                &MAIN_TOOLCHAIN,
                &krate,
                &LogStorage::from(&config),
                EncodingType::Plain,
                || {
                    info!("hello world");
                    Ok(TestResult::TestPass)
                },
            )
            .unwrap();

        // Ensure the data is recorded correctly
        assert_eq!(
            results
                .load_test_result(&ex, &MAIN_TOOLCHAIN, &krate)
                .unwrap(),
            Some(TestResult::TestPass)
        );

        let result_var = results
            .load_log(&ex, &MAIN_TOOLCHAIN, &krate)
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(match result_var {
            EncodedLog::Plain(ref data) => data,
            EncodedLog::Gzip(_) => panic!("The encoded log should not be Gzipped."),
        })
        .contains("hello world"));

        // Ensure no data is returned for missing results
        assert!(results
            .load_test_result(&ex, &TEST_TOOLCHAIN, &krate)
            .unwrap()
            .is_none());
        assert!(results
            .get_result(&ex, &TEST_TOOLCHAIN, &krate)
            .unwrap()
            .is_none());
        assert!(results
            .load_log(&ex, &TEST_TOOLCHAIN, &krate)
            .unwrap()
            .is_none());

        // Add another result
        results
            .record_result(
                &ex,
                &TEST_TOOLCHAIN,
                &krate,
                &LogStorage::from(&config),
                EncodingType::Plain,
                || {
                    info!("Another log message!");
                    Ok(TestResult::TestFail(FailureReason::Unknown))
                },
            )
            .unwrap();

        assert_eq!(
            results.get_result(&ex, &TEST_TOOLCHAIN, &krate).unwrap(),
            Some(TestResult::TestFail(FailureReason::Unknown))
        );

        // Test deleting the newly-added result
        results.delete_result(&ex, &TEST_TOOLCHAIN, &krate).unwrap();
        assert!(results
            .get_result(&ex, &TEST_TOOLCHAIN, &krate)
            .unwrap()
            .is_none());
        assert_eq!(
            results.get_result(&ex, &MAIN_TOOLCHAIN, &krate).unwrap(),
            Some(TestResult::TestPass)
        );

        // Test deleting all the remaining results
        results.delete_all_results(&ex).unwrap();
        assert!(results
            .get_result(&ex, &MAIN_TOOLCHAIN, &krate)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_store() {
        let db = Database::temp().unwrap();
        let results = DatabaseDB::new(&db);
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment to attach the results to
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();
        let ex = Experiment::get(&db, "dummy").unwrap().unwrap();

        let krate = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1".into(),
        });
        let updated = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1.2".into(),
        });

        // Store a result and versions
        results
            .store(
                &ex,
                &ProgressData {
                    result: TaskResult {
                        krate: updated.clone(),
                        toolchain: MAIN_TOOLCHAIN.clone(),
                        result: TestResult::TestPass,
                        log: base64::engine::general_purpose::STANDARD.encode("foo"),
                    },
                    version: Some((krate.clone(), updated.clone())),
                },
                EncodingType::Plain,
            )
            .unwrap();

        assert_eq!(
            results.load_log(&ex, &MAIN_TOOLCHAIN, &updated).unwrap(),
            Some(EncodedLog::Plain(b"foo".to_vec()))
        );
        assert_eq!(
            results
                .load_test_result(&ex, &MAIN_TOOLCHAIN, &updated)
                .unwrap(),
            Some(TestResult::TestPass)
        );

        assert_eq!(
            results.load_log(&ex, &MAIN_TOOLCHAIN, &krate).unwrap(),
            None
        );
        assert_eq!(
            results
                .load_test_result(&ex, &MAIN_TOOLCHAIN, &krate)
                .unwrap(),
            None
        );
    }
}
