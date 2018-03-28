use config::Config;
use crates::{Crate, GitHubRepo};
use errors::*;
use ex::Experiment;
use file;
use report;
use results::{FileDB, TestResult, WriteResults};
use rusoto_core::request::default_tls_client;
use rusoto_s3::S3Client;
use server::tokens::Tokens;
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

pub fn generate_report(ex_name: &str, config: &Config, tokens: &Tokens) -> Result<String> {
    let client = S3Client::new(
        default_tls_client()?,
        tokens.reports_bucket.clone(),
        tokens.reports_bucket.region.clone(),
    );
    let dest = format!("s3://{}/{}", tokens.reports_bucket.bucket, ex_name);
    let writer = report::S3Writer::create(Box::new(client), dest.parse()?)?;

    report::gen(&FileDB::default(), ex_name, &writer, config)?;

    Ok(format!(
        "{}/{}/{}/index.html",
        tokens.reports_bucket.public_url, tokens.reports_bucket.bucket, ex_name
    ))
}
