use errors::*;
use ex::ExCrate;
use ex::Experiment;
use ex::ex_dir;
use file;
use log;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use toolchain::Toolchain;
use util;

pub trait ExperimentResultDB {
    type CrateWriter: CrateResultWriter;
    fn for_crate(&self, crate_: &ExCrate, toolchain: &Toolchain) -> Self::CrateWriter;

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

fn crate_to_dir(c: &ExCrate) -> String {
    match *c {
        ExCrate::Version {
            ref name,
            ref version,
        } => format!("reg/{}-{}", name, version),
        ExCrate::Repo {
            ref org,
            ref name,
            ref sha,
        } => format!("gh/{}.{}.{}", org, name, sha),
    }
}

#[derive(Clone)]
pub struct FileDB<'a> {
    ex: &'a Experiment,
}

impl<'a> FileDB<'a> {
    pub fn for_experiment(ex: &'a Experiment) -> Self {
        FileDB { ex }
    }
}

impl<'a> ExperimentResultDB for FileDB<'a> {
    type CrateWriter = ResultWriter<'a>;
    fn for_crate(&self, crate_: &ExCrate, toolchain: &Toolchain) -> Self::CrateWriter {
        ResultWriter {
            db: self.clone(),
            crate_: crate_.clone(),
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
}

pub struct ResultWriter<'a> {
    db: FileDB<'a>,
    crate_: ExCrate,
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
        PathBuf::from(tc).join(crate_to_dir(&self.crate_))
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
            TestResult::TestPass => "test-pass",
        }.to_string()
    }
}
