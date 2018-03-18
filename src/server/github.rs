use errors::*;
use reqwest::{header, Client, Method, RequestBuilder};
use server::auth::Token;
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
        let url = if !url.starts_with("https://") {
            format!("https://api.github.com/{}", url)
        } else {
            url.to_string()
        };

        let mut req = self.client.request(method, &url);
        req.header(header::Authorization(Token {
            token: self.token.clone(),
        }));
        req
    }

    pub fn username(&self) -> Result<String> {
        let response: User = self.build_request(Method::Get, "user").send()?.json()?;
        Ok(response.login)
    }

    pub fn post_comment(&self, issue_url: &str, body: &str) -> Result<()> {
        self.build_request(Method::Post, &format!("{}/comments", issue_url))
            .json(&json!({
                "body": body,
            }))
            .send()?;
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Deserialize)]
pub struct EventIssueComment {
    pub comment: Comment,
    pub sender: User,
}

#[derive(Deserialize)]
pub struct Comment {
    pub body: String,
    pub issue_url: String,
}
