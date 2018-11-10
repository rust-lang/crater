mod db;
#[cfg(test)]
mod dummy;

use crates::{Crate, GitHubRepo};
use errors::*;
use experiments::Experiment;
pub use results::db::{DatabaseDB, ProgressData};
#[cfg(test)]
pub use results::dummy::DummyDB;
use std::collections::HashMap;
use toolchain::Toolchain;

pub trait ReadResults {
    fn load_all_shas(&self, ex: &Experiment) -> Result<HashMap<GitHubRepo, String>>;
    fn load_log(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<EncodedLog>>;
    fn load_test_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<TestResult>>;
}

pub trait WriteResults {
    fn get_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<TestResult>>;
    fn record_sha(&self, ex: &Experiment, repo: &GitHubRepo, sha: &str) -> Result<()>;
    fn record_result<F>(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        f: F,
        encoding_type: EncodingType,
    ) -> Result<TestResult>
    where
        F: FnOnce() -> Result<TestResult>;
}

pub trait DeleteResults {
    fn delete_all_results(&self, ex: &Experiment) -> Result<()>;
    fn delete_result(&self, ex: &Experiment, toolchain: &Toolchain, krate: &Crate) -> Result<()>;
}

string_enum!(pub enum TestResult {
    BuildFail => "build-fail",
    TestFail => "test-fail",
    TestSkipped => "test-skipped",
    TestPass => "test-pass",
    Error => "error",
});

string_enum!(pub enum EncodingType {
    Plain => "plain",
    Gzip => "gzip",
});

#[derive(Clone, PartialEq, Debug)]
pub enum EncodedLog {
    Plain(Vec<u8>),
    Gzip(Vec<u8>),
}
