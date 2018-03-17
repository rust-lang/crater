use errors::*;
use reqwest::{header, Client, Method, RequestBuilder};
use server::tokens::Tokens;

pub struct GitHubApi {
    token: String,
    client: Client,
}

impl GitHubApi {
    pub fn new(tokens: &Tokens) -> Self {
        GitHubApi {
            token: tokens.bot.api_token.clone(),
            client: Client::new(),
        }
    }

    fn build_request(&self, method: Method, url: &str) -> RequestBuilder {
        let mut req = self.client
            .request(method, &format!("https://api.github.com/{}", url));
        req.header(header::Authorization(format!("token {}", self.token)));
        req
    }

    pub fn username(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Response {
            login: String,
        }

        let response: Response = self.build_request(Method::Get, "user").send()?.json()?;
        Ok(response.login)
    }
}
