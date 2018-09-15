use errors::*;
use reqwest::{header, Client, Method, RequestBuilder, StatusCode};
use server::tokens::Tokens;
use std::collections::HashMap;

lazy_static! {
    static ref USER_AGENT: String = format!("crater/{}", ::GIT_REVISION.unwrap_or("unknown"));
}

#[derive(Clone)]
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
        req.header(header::Authorization(format!("token {}", self.token)));
        req.header(header::UserAgent::new(USER_AGENT.as_str()));
        req
    }

    pub fn username(&self) -> Result<String> {
        let response: User = self.build_request(Method::Get, "user").send()?.json()?;
        Ok(response.login)
    }

    pub fn post_comment(&self, issue_url: &str, body: &str) -> Result<()> {
        let mut response = self
            .build_request(Method::Post, &format!("{}/comments", issue_url))
            .json(&json!({
                "body": body,
            })).send()?;

        if response.status() == StatusCode::Created {
            Ok(())
        } else {
            let error: Error = response.json()?;
            bail!(
                "failed to post comment on issue {} (status code {}): {}",
                issue_url,
                response.status(),
                error.message
            );
        }
    }

    pub fn list_labels(&self, issue_url: &str) -> Result<Vec<Label>> {
        let mut response = self
            .build_request(Method::Get, &format!("{}/labels", issue_url))
            .send()?;

        if response.status() == StatusCode::Ok {
            Ok(response.json()?)
        } else {
            let error: Error = response.json()?;
            bail!(
                "failed to list labels of issue {} (status code {}): {}",
                issue_url,
                response.status(),
                error.message
            );
        }
    }

    pub fn add_label(&self, issue_url: &str, label: &str) -> Result<()> {
        let mut response = self
            .build_request(Method::Post, &format!("{}/labels", issue_url))
            .json(&json!([label]))
            .send()?;

        if response.status() == StatusCode::Ok {
            Ok(())
        } else {
            let error: Error = response.json()?;
            bail!(
                "failed to add label {} to issue {} (status code {}): {}",
                label,
                issue_url,
                response.status(),
                error.message
            );
        }
    }

    pub fn remove_label(&self, issue_url: &str, label: &str) -> Result<()> {
        let mut response = self
            .build_request(Method::Delete, &format!("{}/labels/{}", issue_url, label))
            .send()?;

        if response.status() == StatusCode::Ok {
            Ok(())
        } else {
            let error: Error = response.json()?;
            bail!(
                "failed to remove label {} from issue {} (status code {}): {}",
                label,
                issue_url,
                response.status(),
                error.message
            );
        }
    }

    pub fn list_teams(&self, org: &str) -> Result<HashMap<String, usize>> {
        let mut response = self
            .build_request(Method::Get, &format!("orgs/{}/teams", org))
            .send()?;

        if response.status() == StatusCode::Ok {
            let teams: Vec<Team> = response.json()?;
            Ok(teams.into_iter().map(|t| (t.slug, t.id)).collect())
        } else {
            let error: Error = response.json()?;
            bail!(
                "failed to get {}'s teams (status code {}): {}'",
                org,
                response.status(),
                error.message
            );
        }
    }

    pub fn team_members(&self, team: usize) -> Result<Vec<String>> {
        let mut response = self
            .build_request(Method::Get, &format!("teams/{}/members", team))
            .send()?;

        if response.status() == StatusCode::Ok {
            let users: Vec<User> = response.json()?;
            Ok(users.into_iter().map(|u| u.login).collect())
        } else {
            let error: Error = response.json()?;
            bail!(
                "failed to get team {} members (status code {}): {}'",
                team,
                response.status(),
                error.message
            );
        }
    }
}

#[derive(Deserialize)]
pub struct Error {
    pub message: String,
}

#[derive(Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Deserialize)]
pub struct EventIssueComment {
    pub action: String,
    pub issue: Issue,
    pub comment: Comment,
    pub sender: User,
}

#[derive(Deserialize)]
pub struct Issue {
    pub number: i32,
    pub url: String,
    pub html_url: String,
    pub labels: Vec<Label>,
    pub pull_request: Option<PullRequest>,
}

#[derive(Deserialize)]
pub struct PullRequest {
    pub html_url: String,
}

#[derive(Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Deserialize)]
pub struct Comment {
    pub body: String,
}

#[derive(Deserialize)]
pub struct Team {
    pub id: usize,
    pub slug: String,
}
