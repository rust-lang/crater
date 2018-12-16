use crate::config::Config;
use crate::prelude::*;
use http::header::{HeaderValue, CONTENT_TYPE};
use http::Response;
use http::StatusCode;
use hyper::Body;
use serde::Serialize;
use std::fmt;
use std::fmt::Display;
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
    pub(in crate::server) fn internal_error(error: String) -> ApiResponse<()> {
        ApiResponse::InternalError { error }
    }

    pub(in crate::server) fn unauthorized() -> ApiResponse<()> {
        ApiResponse::Unauthorized
    }

    pub(in crate::server) fn not_found() -> ApiResponse<()> {
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
    pub(in crate::server) fn into_response(self) -> Fallible<Response<Body>> {
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

impl Display for CraterToken {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CraterToken {}", self.token)
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
