use config::Config;
use hyper::header::Scheme;
use hyper::StatusCode;
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
}

impl<T> ApiResponse<T> {
    pub(in server) fn status_code(&self) -> StatusCode {
        match *self {
            ApiResponse::Success { .. } => StatusCode::Ok,
            ApiResponse::InternalError { .. } => StatusCode::InternalServerError,
            ApiResponse::Unauthorized => StatusCode::Unauthorized,
        }
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
