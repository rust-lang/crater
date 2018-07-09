use crates::{Crate, GitHubRepo};
use errors::*;
use ex::{ex_dir, Experiment};
use file;
use log;
use results::{DeleteResults, ReadResults, TestResult, WriteResults};
use serde_json;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use toolchain::Toolchain;
use util;

#[derive(Clone, Default)]
pub struct FileDB {
    shafile_lock: Arc<Mutex<()>>,
}

impl FileDB {
    fn shafile_path(&self, ex: &Experiment) -> PathBuf {
        ex_dir(&ex.name).join("shas.json")
    }

    pub fn result_dir(&self, ex: &Experiment, toolchain: &Toolchain, krate: &Crate) -> PathBuf {
        let crate_path = match *krate {
            Crate::Registry(ref details) => format!("reg/{}-{}", details.name, details.version),
            Crate::GitHub(ref repo) => format!("gh/{}.{}", repo.org, repo.name),
        };

        ex_dir(&ex.name)
            .join("res")
            .join(toolchain.rustup_name())
            .join(crate_path)
    }

    fn result_file(&self, ex: &Experiment, toolchain: &Toolchain, krate: &Crate) -> PathBuf {
        self.result_dir(ex, toolchain, krate).join("results.txt")
    }

    fn result_log(&self, ex: &Experiment, toolchain: &Toolchain, krate: &Crate) -> PathBuf {
        self.result_dir(ex, toolchain, krate).join("log.txt")
    }
}

impl ReadResults for FileDB {
    fn load_all_shas(&self, ex: &Experiment) -> Result<HashMap<GitHubRepo, String>> {
        let path = self.shafile_path(ex);
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let data: Vec<(GitHubRepo, String)> = serde_json::from_str(&file::read_string(&path)?)?;
        Ok(data.into_iter().collect())
    }

    fn load_log(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.result_log(ex, toolchain, krate);

        if path.exists() {
            let mut buffer = Vec::new();
            BufReader::new(File::open(path)?).read_to_end(&mut buffer)?;
            Ok(Some(buffer))
        } else {
            Ok(None)
        }
    }

    fn load_test_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<TestResult>> {
        let path = self.result_file(ex, toolchain, krate);

        if path.exists() {
            let content = file::read_string(&path)?;
            Ok(Some(content.parse()?))
        } else {
            Ok(None)
        }
    }
}

impl WriteResults for FileDB {
    fn get_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<TestResult>> {
        self.load_test_result(ex, toolchain, krate)
    }

    fn record_sha(&self, ex: &Experiment, repo: &GitHubRepo, sha: &str) -> Result<()> {
        // This avoids two threads writing on the same file together
        let _lock = self.shafile_lock.lock().unwrap();

        let mut existing = self.load_all_shas(ex)?;
        existing.insert(repo.clone(), sha.to_string());

        let path = self.shafile_path(ex);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // A Vec is used here instead of an HashMap because JSON doesn't allow non-string keys
        let serializable: Vec<(GitHubRepo, String)> = existing.into_iter().collect();
        file::write_string(&path, &serde_json::to_string(&serializable)?)?;

        Ok(())
    }

    fn record_result<F>(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        f: F,
    ) -> Result<TestResult>
    where
        F: FnOnce() -> Result<TestResult>,
    {
        self.delete_result(ex, toolchain, krate)?;
        fs::create_dir_all(&self.result_dir(ex, toolchain, krate))?;

        let log_file = self.result_log(ex, toolchain, krate);
        let result_file = self.result_file(ex, toolchain, krate);

        let result = log::redirect(&log_file, f)?;
        file::write_string(&result_file, &result.to_string())?;

        Ok(result)
    }
}

impl DeleteResults for FileDB {
    fn delete_all_results(&self, ex: &Experiment) -> Result<()> {
        let dir = ex_dir(&ex.name).join("res");
        if dir.exists() {
            util::remove_dir_all(&dir)?;
        }

        Ok(())
    }

    fn delete_result(&self, ex: &Experiment, toolchain: &Toolchain, krate: &Crate) -> Result<()> {
        let result_dir = self.result_dir(ex, toolchain, krate);
        if result_dir.exists() {
            util::remove_dir_all(&result_dir)?;
        }

        Ok(())
    }
}
