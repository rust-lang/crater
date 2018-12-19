use crate::agent::api::AgentApi;
use crate::crates::{Crate, GitHubRepo};
use crate::experiments::Experiment;
use crate::logs::{self, LogStorage};
use crate::prelude::*;
use crate::results::{TestResult, WriteResults};
use crate::toolchain::Toolchain;
use log::LevelFilter;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

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
    ) -> Fallible<TestResult>
    where
        F: FnOnce() -> Fallible<TestResult>,
    {
        let storage = LogStorage::new(LevelFilter::Info);
        let result = logs::capture(&storage, f)?;
        let output = storage.to_string();

        let shas = ::std::mem::replace(self.shas.lock().unwrap().deref_mut(), Vec::new());

        info!("sending results to the crater server...");
        self.api
            .record_progress(krate, toolchain, output.as_bytes(), result, &shas)?;

        Ok(result)
    }
}
