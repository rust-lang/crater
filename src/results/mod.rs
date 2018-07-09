#[cfg(test)]
mod dummy;
mod file;

use crates::{Crate, GitHubRepo};
use errors::*;
use ex::Experiment;
#[cfg(test)]
pub use results::dummy::DummyDB;
pub use results::file::FileDB;
use std::collections::HashMap;
use toolchain::Toolchain;

pub trait ReadResults {
    fn load_all_shas(&self, ex: &Experiment) -> Result<HashMap<GitHubRepo, String>>;
    fn load_log(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<Vec<u8>>>;
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
