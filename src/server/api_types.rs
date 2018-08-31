use config::Config;
use errors::*;
use http::header::{HeaderValue, CONTENT_TYPE};
use http::Response;
use http::StatusCode;
use hyper::Body;
use reqwest::header::Scheme;
use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AgentConfig {
    pub agent_name: String,
    pub crater_config: Config,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ApiResponse<T> {
    Success { result: T },
    InternalError { error: String },
    Unauthorized,
    NotFound,
}

impl ApiResponse<()> {
    pub(in server) fn internal_error(error: String) -> ApiResponse<()> {
        ApiResponse::InternalError { error }
    }

    pub(in server) fn unauthorized() -> ApiResponse<()> {
        ApiResponse::Unauthorized
    }

    pub(in server) fn not_found() -> ApiResponse<()> {
        ApiResponse::NotFound
    }
}

impl<T> ApiResponse<T> {
    fn status_code(&self) -> StatusCode {
        match *self {
            ApiResponse::Success { .. } => StatusCode::OK,
            ApiResponse::InternalError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiResponse::NotFound => StatusCode::NOT_FOUND,
        }
    }
}

impl<T: Serialize> ApiResponse<T> {
    pub(in server) fn into_response(self) -> Result<Response<Body>> {
        let serialized = ::serde_json::to_vec(&self)?;

        let mut resp = Response::new(serialized.into());
        resp.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        *resp.status_mut() = self.status_code();
        Ok(resp)
    }
}

#[derive(Debug, Clone)]
pub struct CraterToken {
    pub token: String,
}

impl Scheme for CraterToken {
    fn scheme() -> Option<&'static str> {
        Some("CraterToken")
    }

    fn fmt_scheme(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.token)
    }
}

impl FromStr for CraterToken {
    type Err = ::hyper::Error;

    fn from_str(s: &str) -> ::hyper::Result<CraterToken> {
        Ok(CraterToken {
            token: s.to_owned(),
        })
    }
}
