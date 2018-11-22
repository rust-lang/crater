use base64;
use crates::{Crate, GitHubRepo};
use db::{Database, QueryUtils};
use experiments::Experiment;
use flate2::write::GzEncoder;
use flate2::Compression;
use prelude::*;
use results::EncodedLog;
use results::EncodingType;
use results::{DeleteResults, ReadResults, TestResult, WriteResults};
use serde_json;
use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use toolchain::Toolchain;

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
    pub results: Vec<TaskResult>,
    pub shas: Vec<(GitHubRepo, String)>,
}

pub struct DatabaseDB<'a> {
    db: &'a Database,
}

impl<'a> DatabaseDB<'a> {
    pub fn new(db: &'a Database) -> Self {
        DatabaseDB { db }
    }

    pub fn store(
        &self,
        ex: &Experiment,
        data: &ProgressData,
        encoding_type: EncodingType,
    ) -> Fallible<()> {
        for result in &data.results {
            self.store_result(
                ex,
                &result.krate,
                &result.toolchain,
                result.result,
                &base64::decode(&result.log).with_context(|_| "invalid base64 log provided")?,
                encoding_type,
            )?;
        }

        for &(ref repo, ref sha) in &data.shas {
            self.record_sha(ex, repo, sha)?;
        }

        Ok(())
    }

    fn store_result(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        res: TestResult,
        log: &[u8],
        encoding_type: EncodingType,
    ) -> Fallible<()> {
        match encoding_type {
            EncodingType::Gzip => {
                let mut encoded_log = GzEncoder::new(Vec::new(), Compression::default());
                encoded_log.write_all(log).unwrap();
                self.insert_into_results(
                    ex,
                    krate,
                    toolchain,
                    res,
                    encoded_log.finish().unwrap().as_slice(),
                    encoding_type,
                )?;
            }
            EncodingType::Plain => {
                self.insert_into_results(ex, krate, toolchain, res, log, encoding_type)?;
            }
        };

        Ok(())
    }

    fn insert_into_results(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        res: TestResult,
        log: &[u8],
        encoding_type: EncodingType,
    ) -> Fallible<usize> {
        self.db.execute(
            "INSERT INTO results (experiment, crate, toolchain, result, log, encoding) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            &[
                &ex.name,
                &serde_json::to_string(krate)?,
                &toolchain.to_string(),
                &res.to_string(),
                &log,
                &encoding_type.to_str(),
            ],
        )
    }
}

impl<'a> ReadResults for DatabaseDB<'a> {
    fn load_all_shas(&self, ex: &Experiment) -> Fallible<HashMap<GitHubRepo, String>> {
        Ok(self
            .db
            .query(
                "SELECT * FROM shas WHERE experiment = ?1;",
                &[&ex.name],
                |row| {
                    (
                        GitHubRepo {
                            org: row.get("org"),
                            name: row.get("name"),
                        },
                        row.get("sha"),
                    )
                },
            )?.into_iter()
            .collect())
    }

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
            &[
                &ex.name,
                &toolchain.to_string(),
                &serde_json::to_string(krate)?,
            ],
            |row| {
                let log: Vec<u8> = row.get("log");
                let encoding: String = row.get("encoding");
                let encoding = encoding.parse().unwrap();

                match encoding {
                    EncodingType::Plain => EncodedLog::Plain(log),
                    EncodingType::Gzip => EncodedLog::Gzip(log),
                }
            },
        )
    }

    fn load_test_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>> {
        let result: Option<String> = self
            .db
            .query(
                "SELECT result FROM results \
                 WHERE experiment = ?1 AND toolchain = ?2 AND crate = ?3 \
                 LIMIT 1;",
                &[
                    &ex.name,
                    &toolchain.to_string(),
                    &serde_json::to_string(krate)?,
                ],
                |row| row.get("result"),
            )?.pop();

        if let Some(res) = result {
            Ok(Some(res.parse()?))
        } else {
            Ok(None)
        }
    }
}

impl<'a> WriteResults for DatabaseDB<'a> {
    fn get_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>> {
        self.load_test_result(ex, toolchain, krate)
    }

    fn record_sha(&self, ex: &Experiment, repo: &GitHubRepo, sha: &str) -> Fallible<()> {
        self.db.execute(
            "INSERT INTO shas (experiment, org, name, sha) VALUES (?1, ?2, ?3, ?4)",
            &[&ex.name, &repo.org, &repo.name, &sha],
        )?;

        Ok(())
    }

    fn record_result<F>(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        f: F,
        encoding_type: EncodingType,
    ) -> Fallible<TestResult>
    where
        F: FnOnce() -> Fallible<TestResult>,
    {
        let mut log_file = ::tempfile::NamedTempFile::new()?;
        let result = ::log::redirect(log_file.path(), f)?;

        let mut buffer = Vec::new();
        log_file.read_to_end(&mut buffer)?;

        self.store_result(ex, krate, toolchain, result, &buffer, encoding_type)?;

        Ok(result)
    }
}

impl<'a> DeleteResults for DatabaseDB<'a> {
    fn delete_all_results(&self, ex: &Experiment) -> Fallible<()> {
        self.db
            .execute("DELETE FROM results WHERE experiment = ?1;", &[&ex.name])?;
        Ok(())
    }

    fn delete_result(&self, ex: &Experiment, tc: &Toolchain, krate: &Crate) -> Fallible<()> {
        self.db.execute(
            "DELETE FROM results WHERE experiment = ?1 AND toolchain = ?2 AND crate = ?3;",
            &[
                &ex.name,
                &tc.to_string(),
                &serde_json::to_string(krate).unwrap(),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DatabaseDB, ProgressData, TaskResult};
    use actions::CreateExperiment;
    use base64;
    use config::Config;
    use crates::{Crate, GitHubRepo, RegistryCrate};
    use db::Database;
    use experiments::Experiment;
    use results::EncodedLog;
    use results::EncodingType;
    use results::{DeleteResults, FailureReason, ReadResults, TestResult, WriteResults};
    use toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    #[test]
    fn test_shas() {
        let db = Database::temp().unwrap();
        let results = DatabaseDB::new(&db);
        let config = Config::default();

        ::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment to attach the results to
        CreateExperiment::dummy("dummy")
            .apply(&db, &config)
            .unwrap();
        let ex = Experiment::get(&db, "dummy").unwrap().unwrap();

        // Define some dummy GitHub repositories
        let repo1 = GitHubRepo {
            org: "foo".to_string(),
            name: "bar".to_string(),
        };
        let repo2 = GitHubRepo {
            org: "foo".to_string(),
            name: "baz".to_string(),
        };

        // Store some SHAs for those repos
        results
            .record_sha(&ex, &repo1, "0000000000000000000000000000000000000000")
            .unwrap();
        results
            .record_sha(&ex, &repo2, "ffffffffffffffffffffffffffffffffffffffff")
            .unwrap();

        // Ensure all the SHAs were recorded correctly
        let shas = results.load_all_shas(&ex).unwrap();
        assert_eq!(shas.len(), 2);
        assert_eq!(
            shas.get(&repo1).unwrap(),
            "0000000000000000000000000000000000000000"
        );
        assert_eq!(
            shas.get(&repo2).unwrap(),
            "ffffffffffffffffffffffffffffffffffffffff"
        );

        // Ensure results are cleanly overridden when recording the same repo again
        results
            .record_sha(&ex, &repo1, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap();

        let shas = results.load_all_shas(&ex).unwrap();
        assert_eq!(shas.len(), 2);
        assert_eq!(
            shas.get(&repo1).unwrap(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            shas.get(&repo2).unwrap(),
            "ffffffffffffffffffffffffffffffffffffffff"
        );
    }

    #[test]
    fn test_results() {
        let db = Database::temp().unwrap();
        let results = DatabaseDB::new(&db);
        let config = Config::default();

        ::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment to attach the results to
        CreateExperiment::dummy("dummy")
            .apply(&db, &config)
            .unwrap();
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
                || {
                    info!("hello world");
                    Ok(TestResult::TestPass)
                },
                EncodingType::Plain,
            ).unwrap();

        // Ensure the data is recorded correctly
        assert_eq!(
            results
                .load_test_result(&ex, &MAIN_TOOLCHAIN, &krate)
                .unwrap(),
            Some(TestResult::TestPass)
        );
        assert_eq!(
            results.get_result(&ex, &MAIN_TOOLCHAIN, &krate).unwrap(),
            Some(TestResult::TestPass)
        );

        let result_var = results
            .load_log(&ex, &MAIN_TOOLCHAIN, &krate)
            .unwrap()
            .unwrap();
        assert!(
            String::from_utf8_lossy(match result_var {
                EncodedLog::Plain(ref data) => data,
                EncodedLog::Gzip(_) => panic!("The encoded log should not be Gzipped."),
            }).contains("hello world")
        );

        // Ensure no data is returned for missing results
        assert!(
            results
                .load_test_result(&ex, &TEST_TOOLCHAIN, &krate)
                .unwrap()
                .is_none()
        );
        assert!(
            results
                .get_result(&ex, &TEST_TOOLCHAIN, &krate)
                .unwrap()
                .is_none()
        );
        assert!(
            results
                .load_log(&ex, &TEST_TOOLCHAIN, &krate)
                .unwrap()
                .is_none()
        );

        // Add another result
        results
            .record_result(
                &ex,
                &TEST_TOOLCHAIN,
                &krate,
                || {
                    info!("Another log message!");
                    Ok(TestResult::TestFail(FailureReason::Unknown))
                },
                EncodingType::Plain,
            ).unwrap();
        assert_eq!(
            results.get_result(&ex, &TEST_TOOLCHAIN, &krate).unwrap(),
            Some(TestResult::TestFail(FailureReason::Unknown))
        );

        // Test deleting the newly-added result
        results.delete_result(&ex, &TEST_TOOLCHAIN, &krate).unwrap();
        assert!(
            results
                .get_result(&ex, &TEST_TOOLCHAIN, &krate)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            results.get_result(&ex, &MAIN_TOOLCHAIN, &krate).unwrap(),
            Some(TestResult::TestPass)
        );

        // Test deleting all the remaining results
        results.delete_all_results(&ex).unwrap();
        assert!(
            results
                .get_result(&ex, &MAIN_TOOLCHAIN, &krate)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_store() {
        let db = Database::temp().unwrap();
        let results = DatabaseDB::new(&db);
        let config = Config::default();

        ::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment to attach the results to
        CreateExperiment::dummy("dummy")
            .apply(&db, &config)
            .unwrap();
        let ex = Experiment::get(&db, "dummy").unwrap().unwrap();

        let krate = Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "1".into(),
        });

        // Store a result and some SHAs
        results
            .store(
                &ex,
                &ProgressData {
                    results: vec![TaskResult {
                        krate: krate.clone(),
                        toolchain: MAIN_TOOLCHAIN.clone(),
                        result: TestResult::TestPass,
                        log: base64::encode("foo"),
                    }],
                    shas: vec![
                        (
                            GitHubRepo {
                                org: "foo".into(),
                                name: "bar".into(),
                            },
                            "42".into(),
                        ),
                        (
                            GitHubRepo {
                                org: "foo".into(),
                                name: "baz".into(),
                            },
                            "beef".into(),
                        ),
                    ],
                },
                EncodingType::Plain,
            ).unwrap();

        assert_eq!(
            results.load_log(&ex, &MAIN_TOOLCHAIN, &krate).unwrap(),
            Some(EncodedLog::Plain("foo".as_bytes().to_vec()))
        );
        assert_eq!(
            results
                .load_test_result(&ex, &MAIN_TOOLCHAIN, &krate)
                .unwrap(),
            Some(TestResult::TestPass)
        );
    }
}
