use crates::{Crate, GitHubRepo};
use errors::*;
use experiments::Experiment;
use results::{ReadResults, TestResult};
use std::collections::HashMap;
use toolchain::Toolchain;

#[derive(Default)]
struct DummyData {
    shas: HashMap<GitHubRepo, String>,
    logs: HashMap<(Crate, Toolchain), Vec<u8>>,
    results: HashMap<(Crate, Toolchain), TestResult>,
}

#[derive(Default)]
pub struct DummyDB {
    experiments: HashMap<String, DummyData>,
}

impl DummyDB {
    fn get_data(&self, ex: &Experiment) -> Result<&DummyData> {
        Ok(self
            .experiments
            .get(&ex.name)
            .ok_or_else(|| format!("missing experiment {}", ex.name))?)
    }

    pub fn add_dummy_sha(&mut self, ex: &Experiment, repo: GitHubRepo, sha: String) {
        self.experiments
            .entry(ex.name.to_string())
            .or_insert_with(DummyData::default)
            .shas
            .insert(repo, sha);
    }

    pub fn add_dummy_log(&mut self, ex: &Experiment, krate: Crate, tc: Toolchain, log: Vec<u8>) {
        self.experiments
            .entry(ex.name.to_string())
            .or_insert_with(DummyData::default)
            .logs
            .insert((krate, tc), log);
    }

    pub fn add_dummy_result(
        &mut self,
        ex: &Experiment,
        krate: Crate,
        tc: Toolchain,
        res: TestResult,
    ) {
        self.experiments
            .entry(ex.name.to_string())
            .or_insert_with(DummyData::default)
            .results
            .insert((krate, tc), res);
    }
}

impl ReadResults for DummyDB {
    fn load_all_shas(&self, ex: &Experiment) -> Result<HashMap<GitHubRepo, String>> {
        Ok(self.get_data(ex)?.shas.clone())
    }

    fn load_log(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<Vec<u8>>> {
        Ok(self
            .get_data(ex)?
            .logs
            .get(&(krate.clone(), toolchain.clone()))
            .cloned())
    }

    fn load_test_result(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
    ) -> Result<Option<TestResult>> {
        Ok(self
            .get_data(ex)?
            .results
            .get(&(krate.clone(), toolchain.clone()))
            .cloned())
    }
}
