use crates::{Crate, GitHubRepo};
use errors::*;
use ex::Experiment;
use file;
use results::{FileDB, TestResult, WriteResults};
use std::fs;
use toolchain::Toolchain;

#[derive(Deserialize)]
pub struct TaskResult {
    #[serde(rename = "crate")]
    pub krate: Crate,
    pub toolchain: Toolchain,
    pub result: TestResult,
    pub log: String,
    pub shas: Vec<(GitHubRepo, String)>,
}

pub fn store(ex: &Experiment, result: &TaskResult) -> Result<()> {
    let db = FileDB::default();
    let base = db.result_dir(ex, &result.toolchain, &result.krate);

    fs::create_dir_all(&base)?;
    file::write_string(&base.join("log.txt"), &result.log)?;
    file::write_string(&base.join("results.txt"), &result.result.to_string())?;

    for &(ref repo, ref sha) in &result.shas {
        db.record_sha(ex, repo, sha)?;
    }

    Ok(())
}
