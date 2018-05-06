use chrono::{TimeZone, Utc};
use errors::*;
use file;
use rusoto_core::{AwsCredentials, CredentialsError, ProvideAwsCredentials};
use rusoto_core::Region;
use std::collections::HashMap;
use std::path::Path;

static TOKENS_PATH: &'static str = "tokens.toml";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BotTokens {
    pub webhooks_secret: String,
    pub api_token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReportsBucket {
    pub region: Region,
    pub bucket: String,
    pub public_url: String,
    pub access_key: String,
    pub secret_key: String,
}

impl ProvideAwsCredentials for ReportsBucket {
    fn credentials(&self) -> ::std::result::Result<AwsCredentials, CredentialsError> {
        // Let's just hope this code is not used after year 5138
        // - Pietro, 2018
        let expiry = Utc.timestamp(100_000_000_000, 0);

        Ok(AwsCredentials::new(
            self.access_key.clone(),
            self.secret_key.clone(),
            None,
            expiry,
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Tokens {
    pub bot: BotTokens,
    pub reports_bucket: ReportsBucket,
    pub agents: HashMap<String, String>,
}

#[cfg(test)]
impl Default for Tokens {
    fn default() -> Self {
        Tokens {
            bot: BotTokens {
                webhooks_secret: String::new(),
                api_token: String::new(),
            },
            reports_bucket: ReportsBucket {
                region: "us-east-1".parse().unwrap(),
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
    pub fn load() -> Result<Tokens> {
        let content = file::read_string(Path::new(TOKENS_PATH))?;
        let res = ::toml::from_str(&content)?;
        Ok(res)
    }
}
