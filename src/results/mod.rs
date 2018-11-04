mod db;
#[cfg(test)]
mod dummy;

use crates::{Crate, GitHubRepo};
use experiments::Experiment;
use prelude::*;
pub use results::db::{DatabaseDB, ProgressData};
#[cfg(test)]
pub use results::dummy::DummyDB;
use std::collections::HashMap;
use toolchain::Toolchain;

pub trait ReadResults {
    fn load_all_shas(&self, ex: &Experiment) -> Fallible<HashMap<GitHubRepo, String>>;
    fn load_log(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<Vec<u8>>>;
    fn load_test_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>>;
}

pub trait WriteResults {
    fn get_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Fallible<Option<TestResult>>;
    fn record_sha(&self, ex: &Experiment, repo: &GitHubRepo, sha: &str) -> Fallible<()>;
    fn record_result<F>(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        f: F,
    ) -> Fallible<TestResult>
    where
        F: FnOnce() -> Fallible<TestResult>;
}

pub trait DeleteResults {
    fn delete_all_results(&self, ex: &Experiment) -> Fallible<()>;
    fn delete_result(&self, ex: &Experiment, toolchain: &Toolchain, krate: &Crate) -> Fallible<()>;
}

string_enum!(pub enum TestResult {
    BuildFail => "build-fail",
    TestFail => "test-fail",
    TestSkipped => "test-skipped",
    TestPass => "test-pass",
    Error => "error",
});
