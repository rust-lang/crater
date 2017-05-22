use errors::*;
use ex::ExCrate;
use ex::ex_dir;
use file;
use gh_mirrors;
use log;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use toolchain::Toolchain;
use util;


fn crate_to_dir(c: &ExCrate) -> String {
    match *c {
        ExCrate::Version {
            ref name,
            ref version,
        } => format!("reg/{}-{}", name, version),
        ExCrate::Repo { ref url, ref sha } => {
            let (org, name) = gh_mirrors::gh_url_to_org_and_name(url)
                .expect("malformed github repo name");
            format!("gh/{}.{}.{}", org, name, sha)
        }
    }
}

pub struct ResultWriter<'a> {
    ex_name: &'a str,
    crate_: &'a ExCrate,
    toolchain: &'a Toolchain,
}

impl<'a> ResultWriter<'a> {
    pub fn new(ex_name: &'a str, crate_: &'a ExCrate, toolchain: &'a Toolchain) -> Self {
        Self {
            ex_name,
            crate_,
            toolchain,
        }
    }

    fn init(&self) -> Result<()> {
        self.delete_result()?;
        fs::create_dir_all(&self.result_dir())?;
        Ok(())
    }

    pub fn delete_result(&self) -> Result<()> {
        let result_dir = self.result_dir();
        if result_dir.exists() {
            util::remove_dir_all(&result_dir)?;
        }
        Ok(())
    }

    /// Return a path fragement that can be used to identify this crate and
    /// toolchain.
    pub fn result_path_fragement(&self) -> PathBuf {
        let tc = self.toolchain.rustup_name();
        PathBuf::from(tc).join(crate_to_dir(self.crate_))
    }

    fn result_dir(&self) -> PathBuf {
        ex_dir(self.ex_name)
            .join("res")
            .join(self.result_path_fragement())
    }

    fn result_file(&self) -> PathBuf {
        self.result_dir().join("results.txt")
    }

    fn result_log(&self) -> PathBuf {
        self.result_dir().join("log.txt")
    }

    pub fn read_log(&self) -> Result<fs::File> {
        fs::File::open(self.result_log()).chain_err(|| "Couldn't open result file.")
    }

    pub fn record_results<F>(&self, f: F) -> Result<TestResult>
        where F: FnOnce() -> Result<TestResult>
    {
        self.init()?;
        let log_file = self.result_log();
        let result_file = self.result_file();

        let result = log::redirect(&log_file, f)?;
        file::write_string(&result_file, &result.to_string())?;

        Ok(result)
    }

    pub fn get_test_results(&self) -> Result<Option<TestResult>> {
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
            }
            .to_string()
    }
}
