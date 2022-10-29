use crate::prelude::*;
use serde_derive::Deserialize;
use std::collections::HashMap;
use std::path::Path;

static TOKENS_PATH: &str = "tokens.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BucketRegion {
    S3 { region: String },
    Custom { url: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BotTokens {
    pub webhooks_secret: String,
    pub api_token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReportsBucket {
    pub region: BucketRegion,
    pub bucket: String,
    pub public_url: String,
    pub access_key: String,
    pub secret_key: String,
}

impl ReportsBucket {
    pub(crate) fn to_aws_credentials(&self) -> aws_types::credentials::SharedCredentialsProvider {
        aws_types::credentials::SharedCredentialsProvider::new(aws_types::Credentials::from_keys(
            self.access_key.clone(),
            self.secret_key.clone(),
            None,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Tokens {
    #[serde(default)]
    pub bot: Option<BotTokens>,
    pub reports_bucket: ReportsBucket,
    pub agents: HashMap<String, String>,
}

#[cfg(test)]
impl Default for Tokens {
    fn default() -> Self {
        Tokens {
            bot: None,
            reports_bucket: ReportsBucket {
                region: BucketRegion::S3 {
                    region: "us-west-1".to_string(),
                },
                bucket: "crater-reports".into(),
                public_url: String::new(),
                access_key: String::new(),
                secret_key: String::new(),
            },
            agents: HashMap::new(),
        }
    }
}

impl Tokens {
    pub fn load() -> Fallible<Tokens> {
        let content = ::std::fs::read_to_string(Path::new(TOKENS_PATH))
            .with_context(|_| format!("could not find {TOKENS_PATH}"))?;
        let res = ::toml::from_str(&content)?;
        Ok(res)
    }
}
