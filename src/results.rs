use crates::{Crate, GitHubRepo};
use errors::*;
use ex::{ex_dir, Experiment};
use file;
use log;
use serde_json;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use toolchain::Toolchain;
use util;

pub trait ExperimentResultDB {
    type CrateWriter: CrateResultWriter;
    fn for_crate(&self, krate: &Crate, toolchain: &Toolchain) -> Self::CrateWriter;

    fn record_sha(&self, repo: &GitHubRepo, sha: &str) -> Result<()>;
    fn load_all_shas(&self) -> Result<HashMap<GitHubRepo, String>>;
    fn delete_all_results(&self) -> Result<()>;
}

pub trait CrateResultWriter {
    /// Return a path fragement that can be used to identify this crate and
    /// toolchain.
    fn result_path_fragement(&self) -> PathBuf;

    fn record_results<F>(&self, f: F) -> Result<TestResult>
    where
        F: FnOnce() -> Result<TestResult>;
    fn load_test_result(&self) -> Result<Option<TestResult>>;
    fn read_log(&self) -> Result<fs::File>;
    fn delete_result(&self) -> Result<()>;
}

fn crate_to_dir(c: &Crate) -> String {
    match *c {
        Crate::Registry(ref details) => format!("reg/{}-{}", details.name, details.version),
        Crate::GitHub(ref repo) => format!("gh/{}.{}", repo.org, repo.name),
    }
}

#[derive(Clone)]
pub struct FileDB<'a> {
    ex: &'a Experiment,
    shafile_lock: Arc<Mutex<()>>,
}

impl<'a> FileDB<'a> {
    pub fn for_experiment(ex: &'a Experiment) -> Self {
        FileDB {
            ex,
            shafile_lock: Arc::new(Mutex::new(())),
        }
    }

    fn shafile_path(&self) -> PathBuf {
        ex_dir(&self.ex.name).join("res").join("shas.json")
    }
}

impl<'a> ExperimentResultDB for FileDB<'a> {
    type CrateWriter = ResultWriter<'a>;
    fn for_crate(&self, krate: &Crate, toolchain: &Toolchain) -> Self::CrateWriter {
        ResultWriter {
            db: self.clone(),
            krate: krate.clone(),
            toolchain: toolchain.clone(),
        }
    }

    fn delete_all_results(&self) -> Result<()> {
        let dir = ex_dir(&self.ex.name).join("res");
        if dir.exists() {
            util::remove_dir_all(&dir)?;
        }

        Ok(())
    }

    fn record_sha(&self, repo: &GitHubRepo, sha: &str) -> Result<()> {
        // This avoids two threads writing on the same file together
        let _lock = self.shafile_lock.lock().unwrap();

        let mut existing = self.load_all_shas()?;
        existing.insert(repo.clone(), sha.to_string());

        let path = self.shafile_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // A Vec is used here instead of an HashMap because JSON doesn't allow non-string keys
        let serializable: Vec<(GitHubRepo, String)> = existing.into_iter().collect();
        file::write_string(&path, &serde_json::to_string(&serializable)?)?;

        Ok(())
    }

    fn load_all_shas(&self) -> Result<HashMap<GitHubRepo, String>> {
        let path = self.shafile_path();
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let data: Vec<(GitHubRepo, String)> = serde_json::from_str(&file::read_string(&path)?)?;
        Ok(data.into_iter().collect())
    }
}

pub struct ResultWriter<'a> {
    db: FileDB<'a>,
    krate: Crate,
    toolchain: Toolchain,
}

impl<'a> CrateResultWriter for ResultWriter<'a> {
    fn delete_result(&self) -> Result<()> {
        let result_dir = self.result_dir();
        if result_dir.exists() {
            util::remove_dir_all(&result_dir)?;
        }
        Ok(())
    }

    /// Return a path fragement that can be used to identify this crate and
    /// toolchain.
    fn result_path_fragement(&self) -> PathBuf {
        let tc = self.toolchain.rustup_name();
        PathBuf::from(tc).join(crate_to_dir(&self.krate))
    }

    fn read_log(&self) -> Result<fs::File> {
        fs::File::open(self.result_log()).chain_err(|| "Couldn't open result file.")
    }

    fn record_results<F>(&self, f: F) -> Result<TestResult>
    where
        F: FnOnce() -> Result<TestResult>,
    {
        self.init()?;
        let log_file = self.result_log();
        let result_file = self.result_file();

        let result = log::redirect(&log_file, f)?;
        file::write_string(&result_file, &result.to_string())?;

        Ok(result)
    }

    fn load_test_result(&self) -> Result<Option<TestResult>> {
        let result_file = self.result_file();
        if result_file.exists() {
            let s = file::read_string(&result_file)?;
            let r = s.parse::<TestResult>()
                .chain_err(|| format!("invalid test result value: '{}'", s))?;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }
}

impl<'a> ResultWriter<'a> {
    fn init(&self) -> Result<()> {
        self.delete_result()?;
        fs::create_dir_all(&self.result_dir())?;
        Ok(())
    }

    fn result_dir(&self) -> PathBuf {
        ex_dir(&self.db.ex.name)
            .join("res")
            .join(self.result_path_fragement())
    }

    fn result_file(&self) -> PathBuf {
        self.result_dir().join("results.txt")
    }

    fn result_log(&self) -> PathBuf {
        self.result_dir().join("log.txt")
    }
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum TestResult {
    BuildFail,
    TestFail,
    TestSkipped,
    TestPass,
}
impl Display for TestResult {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.to_string().fmt(f)
    }
}

impl FromStr for TestResult {
    type Err = Error;

    fn from_str(s: &str) -> Result<TestResult> {
        match s {
            "build-fail" => Ok(TestResult::BuildFail),
            "test-fail" => Ok(TestResult::TestFail),
            "test-skipped" => Ok(TestResult::TestSkipped),
            "test-pass" => Ok(TestResult::TestPass),
            _ => Err(format!("bogus test result: {}", s).into()),
        }
    }
}

impl TestResult {
    fn to_string(&self) -> String {
        match *self {
            TestResult::BuildFail => "build-fail",
            TestResult::TestFail => "test-fail",
            TestResult::TestSkipped => "test-skipped",
            TestResult::TestPass => "test-pass",
        }.to_string()
    }
}
