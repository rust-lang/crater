use config::Config;
use hyper::StatusCode;

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
