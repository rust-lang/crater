use agent::api::AgentApi;
use crates::{Crate, GitHubRepo};
use experiments::Experiment;
use log;
use prelude::*;
use results::EncodingType;
use results::{TestResult, WriteResults};
use std::io::Read;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use toolchain::Toolchain;

#[derive(Clone)]
pub struct ResultsUploader<'a> {
    api: &'a AgentApi,
    shas: Arc<Mutex<Vec<(GitHubRepo, String)>>>,
}

impl<'a> ResultsUploader<'a> {
    pub fn new(api: &'a AgentApi) -> Self {
        ResultsUploader {
            api,
            shas: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl<'a> WriteResults for ResultsUploader<'a> {
    fn get_result(
        &self,
        _ex: &Experiment,
        _toolchain: &Toolchain,
        _krate: &Crate,
    ) -> Fallible<Option<TestResult>> {
        // TODO: not yet implemented
        Ok(None)
    }

    fn record_sha(&self, _ex: &Experiment, repo: &GitHubRepo, sha: &str) -> Fallible<()> {
        self.shas
            .lock()
            .unwrap()
            .push((repo.clone(), sha.to_string()));
        Ok(())
    }

    fn record_result<F>(
        &self,
        _ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        f: F,
        _: EncodingType,
    ) -> Fallible<TestResult>
    where
        F: FnOnce() -> Fallible<TestResult>,
    {
        let mut log_file = ::tempfile::NamedTempFile::new()?;
        let result = log::redirect(log_file.path(), f)?;

        let mut buffer = Vec::new();
        log_file.read_to_end(&mut buffer)?;

        let shas = ::std::mem::replace(self.shas.lock().unwrap().deref_mut(), Vec::new());

        info!("sending results to the crater server...");
        self.api
            .record_progress(krate, toolchain, &buffer, result, &shas)?;

        Ok(result)
    }
}
