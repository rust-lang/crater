use crate::agent::api::AgentApi;
use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::{EncodingType, TestResult, WriteResults};
use crate::toolchain::Toolchain;
use rustwide::logging::{self, LogStorage};
use std::collections::{hash_map::Entry::Occupied, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ResultsUploader<'a> {
    api: &'a AgentApi,
    versions: Arc<Mutex<HashMap<Crate, (Crate, bool)>>>,
}

impl<'a> ResultsUploader<'a> {
    pub fn new(api: &'a AgentApi) -> Self {
        ResultsUploader {
            api,
            versions: Arc::new(Mutex::new(HashMap::new())),
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

    fn update_crate_version(&self, _ex: &Experiment, old: &Crate, new: &Crate) -> Fallible<()> {
        self.versions
            .lock()
            .unwrap()
            .insert(old.clone(), (new.clone(), false));
        Ok(())
    }

    fn record_result<F>(
        &self,
        ex: &Experiment,
        toolchain: &Toolchain,
        krate: &Crate,
        existing_logs: Option<LogStorage>,
        config: &Config,
        _: EncodingType,
        f: F,
    ) -> Fallible<TestResult>
    where
        F: FnOnce() -> Fallible<TestResult>,
    {
        let storage = existing_logs.unwrap_or_else(|| LogStorage::from(config));
        let result = logging::capture(&storage, f)?;
        let output = storage.to_string();

        let mut updated = None;
        let mut new_version = None;
        // This is done to avoid locking versions if record_progress retries in loop
        {
            let mut versions = self.versions.lock().unwrap();
            if let Occupied(mut entry) = versions.entry(krate.clone()) {
                let value = entry.get_mut();

                if value.1 {
                    // delete entry if we already processed both toolchains
                    updated = Some(entry.remove().0);
                } else {
                    updated = Some(value.0.clone());
                    new_version = updated.as_ref();
                    // mark we already sent the updated version to the server
                    value.1 = true;
                }
            };
        }

        info!("sending results to the crater server...");
        self.api.record_progress(
            ex,
            updated.as_ref().unwrap_or(krate),
            toolchain,
            output.as_bytes(),
            &result,
            new_version.map(|new| (krate, new)),
        )?;

        Ok(result)
    }
}
