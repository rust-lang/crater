use crate::prelude::*;
use crate::server::tokens::Tokens;
use crate::utils;
use http::header::AUTHORIZATION;
use http::Method;
use http::StatusCode;
use reqwest::RequestBuilder;
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Fail)]
pub enum GitHubError {
    #[fail(display = "request to GitHub API failed with status {}: {}", _0, _1)]
    RequestFailed(StatusCode, String),
}

#[derive(Clone)]
pub struct GitHubApi {
    token: String,
}

impl GitHubApi {
    pub fn new(tokens: &Tokens) -> Self {
        GitHubApi {
            token: tokens.bot.api_token.clone(),
        }
    }

    fn build_request(&self, method: Method, url: &str) -> RequestBuilder {
        let url = if !url.starts_with("https://") {
            format!("https://api.github.com/{}", url)
        } else {
            url.to_string()
        };

        utils::http::prepare_sync(method, &url)
            .header(AUTHORIZATION, format!("token {}", self.token))
    }

    pub fn username(&self) -> Fallible<String> {
        let response: User = self.build_request(Method::GET, "user").send()?.json()?;
        Ok(response.login)
    }

    pub fn post_comment(&self, issue_url: &str, body: &str) -> Fallible<()> {
        let mut response = self
            .build_request(Method::POST, &format!("{}/comments", issue_url))
            .json(&json!({
                "body": body,
            }))
            .send()?;

        if response.status() == StatusCode::CREATED {
            Ok(())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(response.status(), error.message).into())
        }
    }

    pub fn list_labels(&self, issue_url: &str) -> Fallible<Vec<Label>> {
        let mut response = self
            .build_request(Method::GET, &format!("{}/labels", issue_url))
            .send()?;

        if response.status() == StatusCode::OK {
            Ok(response.json()?)
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(response.status(), error.message).into())
        }
    }

    pub fn add_label(&self, issue_url: &str, label: &str) -> Fallible<()> {
        let mut response = self
            .build_request(Method::POST, &format!("{}/labels", issue_url))
            .json(&json!([label]))
            .send()?;

        if response.status() == StatusCode::OK {
            Ok(())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(response.status(), error.message).into())
        }
    }

    pub fn remove_label(&self, issue_url: &str, label: &str) -> Fallible<()> {
        let mut response = self
            .build_request(Method::DELETE, &format!("{}/labels/{}", issue_url, label))
            .send()?;

        if response.status() == StatusCode::OK {
            Ok(())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(response.status(), error.message).into())
        }
    }

    pub fn list_teams(&self, org: &str) -> Fallible<HashMap<String, usize>> {
        let mut response = self
            .build_request(Method::GET, &format!("orgs/{}/teams", org))
            .send()?;

        if response.status() == StatusCode::OK {
            let teams: Vec<Team> = response.json()?;
            Ok(teams.into_iter().map(|t| (t.slug, t.id)).collect())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(response.status(), error.message).into())
        }
    }

    pub fn team_members(&self, team: usize) -> Fallible<Vec<String>> {
        let mut response = self
            .build_request(Method::GET, &format!("teams/{}/members", team))
            .send()?;

        if response.status() == StatusCode::OK {
            let users: Vec<User> = response.json()?;
            Ok(users.into_iter().map(|u| u.login).collect())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(response.status(), error.message).into())
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
