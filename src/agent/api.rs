use errors::*;
use reqwest::{header, Client, Method, RequestBuilder, StatusCode};

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub agent_name: String,
}

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
        req.header(header::Authorization(format!("token {}", self.token)));
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
                            ::std::thread::sleep(::std::time::Duration::from_secs(10));
                            continue;
                        }
                    }
                    return Err(err);
                }
            }
        }
    }

    pub fn config(&self) -> Result<Config> {
        self.retry(|this| {
            let mut resp = this.build_request(Method::Get, "config").send()?;
            match resp.status() {
                StatusCode::Ok => Ok(resp.json()?),
                StatusCode::Unauthorized => bail!("invalid authorization token!"),
                s => bail!("received {} status code from the crater server", s),
            }
        })
    }
}
