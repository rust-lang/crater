use crate::agent::Capabilities;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::TestResult;
use crate::server::api_types::{AgentConfig, ApiResponse, CraterToken};
use crate::toolchain::Toolchain;
use crate::utils;
use http::{header::AUTHORIZATION, Method, StatusCode};
use reqwest::RequestBuilder;
use serde::de::DeserializeOwned;
use serde_json::json;

#[derive(Debug, Fail)]
pub enum AgentApiError {
    #[fail(display = "invalid API endpoint called")]
    InvalidEndpoint,
    #[fail(display = "Crater server unavailable")]
    ServerUnavailable,
    #[fail(display = "payload sent to the server too large")]
    PayloadTooLarge,
    #[fail(display = "invalid authorization token")]
    InvalidAuthorizationToken,
    #[fail(display = "internal server error: {}", _0)]
    InternalServerError(String),
}

trait ResponseExt {
    fn to_api_response<T: DeserializeOwned>(self) -> Fallible<T>;
}

impl ResponseExt for ::reqwest::Response {
    fn to_api_response<T: DeserializeOwned>(mut self) -> Fallible<T> {
        // 404 responses are not JSON, so avoid parsing them
        match self.status() {
            StatusCode::NOT_FOUND => return Err(AgentApiError::InvalidEndpoint.into()),
            StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT => {
                return Err(AgentApiError::ServerUnavailable.into());
            }
            StatusCode::PAYLOAD_TOO_LARGE => return Err(AgentApiError::PayloadTooLarge.into()),
            _ => {}
        }

        let result: ApiResponse<T> = self.json().with_context(|_| {
            format!(
                "failed to parse API response (status code {})",
                self.status()
            )
        })?;
        match result {
            ApiResponse::Success { result } => Ok(result),
            ApiResponse::InternalError { error } => {
                Err(AgentApiError::InternalServerError(error).into())
            }
            ApiResponse::Unauthorized => Err(AgentApiError::InvalidAuthorizationToken.into()),
            ApiResponse::NotFound => Err(AgentApiError::InvalidEndpoint.into()),
        }
    }
}

const RETRY_AFTER: u64 = 5;

pub struct AgentApi {
    url: String,
    token: String,
}

impl AgentApi {
    pub fn new(url: &str, token: &str) -> Self {
        AgentApi {
            url: url.to_string(),
            token: token.to_string(),
        }
    }

    fn build_request(&self, method: Method, url: &str) -> RequestBuilder {
        utils::http::prepare_sync(method, &format!("{}/agent-api/{}", self.url, url)).header(
            AUTHORIZATION,
            (CraterToken {
                token: self.token.clone(),
            })
            .to_string(),
        )
    }

    fn retry<T, F: Fn(&Self) -> Fallible<T>>(&self, f: F) -> Fallible<T> {
        loop {
            match f(self) {
                Ok(res) => return Ok(res),
                Err(err) => {
                    let retry = if let Some(AgentApiError::ServerUnavailable) = err.downcast_ref() {
                        true
                    } else if let Some(err) = err.downcast_ref::<::reqwest::Error>() {
                        let reqwest_io = err
                            .get_ref()
                            .map(|inner| inner.is::<::std::io::Error>())
                            .unwrap_or(false);
                        let hyper_io = err
                            .get_ref()
                            .and_then(|inner| inner.downcast_ref::<::hyper::Error>())
                            .and_then(|inner| inner.cause2())
                            .map(|inner| inner.is::<::std::io::Error>())
                            .unwrap_or(false);
                        reqwest_io || hyper_io
                    } else {
                        false
                    };

                    if retry {
                        warn!("connection to the server failed. retrying in a few seconds...");
                        ::std::thread::sleep(::std::time::Duration::from_secs(RETRY_AFTER));
                        continue;
                    }

                    return Err(err);
                }
            }
        }
    }

    pub fn config(&self, caps: &Capabilities) -> Fallible<AgentConfig> {
        self.retry(|this| {
            this.build_request(Method::POST, "config")
                .json(&json!(caps))
                .send()?
                .to_api_response()
        })
    }

    pub fn next_experiment(&self) -> Fallible<(Experiment, Vec<Crate>)> {
        self.retry(|this| loop {
            let resp: Option<_> = this
                .build_request(Method::GET, "next-experiment")
                .send()?
                .to_api_response()?;

            if let Some((experiment, crates)) = resp {
                return Ok((experiment, crates));
            }

            ::std::thread::sleep(::std::time::Duration::from_secs(RETRY_AFTER));
        })
    }

    pub fn record_progress(
        &self,
        ex: &Experiment,
        krate: &Crate,
        toolchain: &Toolchain,
        log: &[u8],
        result: &TestResult,
        version: Option<(&Crate, &Crate)>,
    ) -> Fallible<()> {
        self.retry(|this| {
            let _: bool = this
                .build_request(Method::POST, "record-progress")
                .json(&json!({
                    "experiment-name": ex.name,
                    "results": [
                        {
                            "crate": krate,
                            "toolchain": toolchain,
                            "result": result,
                            "log": base64::encode(log),
                        },
                    ],
                    "version": version
                }))
                .send()?
                .to_api_response()?;
            Ok(())
        })
    }

    pub fn heartbeat(&self) -> Fallible<()> {
        self.retry(|this| {
            let _: bool = this
                .build_request(Method::POST, "heartbeat")
                .send()?
                .to_api_response()?;
            Ok(())
        })
    }

    pub fn report_error(&self, ex: &Experiment, error: String) -> Fallible<()> {
        self.retry(|this| {
            let _: bool = this
                .build_request(Method::POST, "error")
                .json(&json!({
                    "experiment-name": ex.name,
                    "error": error
                }))
                .send()?
                .to_api_response()?;
            Ok(())
        })
    }
}
