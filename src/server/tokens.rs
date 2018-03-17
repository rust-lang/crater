use errors::*;
use file;
use std::path::Path;

static TOKENS_PATH: &'static str = "tokens.toml";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BotTokens {
    pub webhooks_secret: String,
    pub api_token: String,
}

#[derive(Debug, Deserialize)]
pub struct Tokens {
    pub bot: BotTokens,
}

impl Tokens {
    pub fn load() -> Result<Tokens> {
        let content = file::read_string(Path::new(TOKENS_PATH))?;
        let res = ::toml::from_str(&content)?;
        Ok(res)
    }
}
