use base64;
use crates::{Crate, GitHubRepo};
use errors::*;
use ex::Experiment;
use reqwest::{header, Client, Method, RequestBuilder, StatusCode};
use results::TestResult;
use serde::de::DeserializeOwned;
use server::api_types::{AgentConfig, ApiResponse, CraterToken};
use toolchain::Toolchain;

trait ResponseExt {
    fn to_api_response<T: DeserializeOwned>(self) -> Result<T>;
}

impl ResponseExt for ::reqwest::Response {
    fn to_api_response<T: DeserializeOwned>(mut self) -> Result<T> {
        // 404 responses are not JSON, so avoid parsing them
        if self.status() == StatusCode::NotFound {
            bail!("invalid API endpoint called");
        }

        let result: ApiResponse<T> = self.json().chain_err(|| "failed to parse API response")?;
        match result {
            ApiResponse::Success { result } => Ok(result),
            ApiResponse::InternalError { error } => bail!("internal server error: {}", error),
            ApiResponse::Unauthorized => bail!("invalid authorization token provided"),
        }
    }
}

const RETRY_AFTER: u64 = 5;

pub struct AgentApi {
    client: Client,
    url: String,
    token: String,
}

impl AgentApi {
    pub fn new(url: &str, token: &str) -> Self {
        AgentApi {
            client: Client::new(),
            url: url.to_string(),
            token: token.to_string(),
        }
    }

    fn build_request(&self, method: Method, url: &str) -> RequestBuilder {
        let mut req = self.client
            .request(method, &format!("{}/agent-api/{}", self.url, url));
        req.header(header::Authorization(CraterToken {
            token: self.token.clone(),
        }));
        req
    }

    fn retry<T, F: Fn(&Self) -> Result<T>>(&self, f: F) -> Result<T> {
        loop {
            match f(self) {
                Ok(res) => return Ok(res),
                Err(err) => {
                    if let ErrorKind::ReqwestError(ref error) = *err.kind() {
                        if error
                            .get_ref()
                            .map(|e| e.is::<::std::io::Error>())
                            .unwrap_or(false)
                        {
                            warn!("connection to the server failed. retrying in a few seconds...");
                            ::std::thread::sleep(::std::time::Duration::from_secs(RETRY_AFTER));
                            continue;
                        }
                    }
                    return Err(err);
                }
            }
        }
    }

    pub fn config(&self) -> Result<AgentConfig> {
        self.retry(|this| {
            this.build_request(Method::Get, "config")
                .send()?
                .to_api_response()
        })
    }

    pub fn next_experiment(&self) -> Result<Experiment> {
        self.retry(|this| loop {
            let resp: Option<_> = this.build_request(Method::Get, "next-experiment")
                .send()?
                .to_api_response()?;

            if let Some(experiment) = resp {
                return Ok(experiment);
            }

            ::std::thread::sleep(::std::time::Duration::from_secs(RETRY_AFTER));
        })
    }

    pub fn record_progress(
        &self,
        krate: &Crate,
        toolchain: &Toolchain,
        log: &[u8],
        result: TestResult,
        shas: &[(GitHubRepo, String)],
    ) -> Result<()> {
        self.retry(|this| {
            let _: bool = this.build_request(Method::Post, "record-progress")
                .json(&json!({
                    "results": [
                        {
                            "crate": krate,
                            "toolchain": toolchain,
                            "result": result,
                            "log": base64::encode(log),
                        },
                    ],
                    "shas": shas,
                }))
                .send()?
                .to_api_response()?;
            Ok(())
        })
    }

    pub fn complete_experiment(&self) -> Result<()> {
        self.retry(|this| {
            let _: bool = this.build_request(Method::Post, "complete-experiment")
                .send()?
                .to_api_response()?;
            Ok(())
        })
    }

    pub fn heartbeat(&self) -> Result<()> {
        self.retry(|this| {
            let _: bool = this.build_request(Method::Post, "heartbeat")
                .send()?
                .to_api_response()?;
            Ok(())
        })
    }
}
