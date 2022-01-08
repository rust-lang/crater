use crate::prelude::*;
use crate::server::tokens::BotTokens;
use crate::utils;
use reqwest::blocking::RequestBuilder;
use reqwest::header::AUTHORIZATION;
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Fail)]
pub enum GitHubError {
    #[fail(display = "request to GitHub API failed with status {}: {}", _0, _1)]
    RequestFailed(StatusCode, String),
}

pub trait GitHub {
    fn username(&self) -> Fallible<String>;
    fn post_comment(&self, issue_url: &str, body: &str) -> Fallible<()>;
    fn list_labels(&self, issue_url: &str) -> Fallible<Vec<Label>>;
    fn add_label(&self, issue_url: &str, label: &str) -> Fallible<()>;
    fn remove_label(&self, issue_url: &str, label: &str) -> Fallible<()>;
    fn list_teams(&self, org: &str) -> Fallible<HashMap<String, usize>>;
    fn team_members(&self, team: usize) -> Fallible<Vec<String>>;
    fn get_commit(&self, repo: &str, sha: &str) -> Fallible<Commit>;
    fn get_pr_head_sha(&self, repo: &str, pr: i32) -> Fallible<String>;
}

#[derive(Clone)]
pub struct GitHubApi {
    token: String,
}

impl GitHubApi {
    pub fn new(tokens: &BotTokens) -> Self {
        GitHubApi {
            token: tokens.api_token.clone(),
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
}

impl GitHub for GitHubApi {
    fn username(&self) -> Fallible<String> {
        let response: User = self.build_request(Method::GET, "user").send()?.json()?;
        Ok(response.login)
    }

    fn post_comment(&self, issue_url: &str, body: &str) -> Fallible<()> {
        let response = self
            .build_request(Method::POST, &format!("{}/comments", issue_url))
            .json(&json!({
                "body": body,
            }))
            .send()?;

        let status = response.status();
        if status == StatusCode::CREATED {
            Ok(())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(status, error.message).into())
        }
    }

    fn list_labels(&self, issue_url: &str) -> Fallible<Vec<Label>> {
        let response = self
            .build_request(Method::GET, &format!("{}/labels", issue_url))
            .send()?;

        let status = response.status();
        if status == StatusCode::OK {
            Ok(response.json()?)
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(status, error.message).into())
        }
    }

    fn add_label(&self, issue_url: &str, label: &str) -> Fallible<()> {
        let response = self
            .build_request(Method::POST, &format!("{}/labels", issue_url))
            .json(&json!([label]))
            .send()?;

        let status = response.status();
        if status == StatusCode::OK {
            Ok(())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(status, error.message).into())
        }
    }

    fn remove_label(&self, issue_url: &str, label: &str) -> Fallible<()> {
        let response = self
            .build_request(Method::DELETE, &format!("{}/labels/{}", issue_url, label))
            .send()?;

        let status = response.status();
        if status == StatusCode::OK {
            Ok(())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(status, error.message).into())
        }
    }

    fn list_teams(&self, org: &str) -> Fallible<HashMap<String, usize>> {
        let response = self
            .build_request(Method::GET, &format!("orgs/{}/teams", org))
            .send()?;

        let status = response.status();
        if status == StatusCode::OK {
            let teams: Vec<Team> = response.json()?;
            Ok(teams.into_iter().map(|t| (t.slug, t.id)).collect())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(status, error.message).into())
        }
    }

    fn team_members(&self, team: usize) -> Fallible<Vec<String>> {
        let response = self
            .build_request(Method::GET, &format!("teams/{}/members", team))
            .send()?;

        let status = response.status();
        if status == StatusCode::OK {
            let users: Vec<User> = response.json()?;
            Ok(users.into_iter().map(|u| u.login).collect())
        } else {
            let error: Error = response.json()?;
            Err(GitHubError::RequestFailed(status, error.message).into())
        }
    }

    fn get_commit(&self, repo: &str, sha: &str) -> Fallible<Commit> {
        let commit = self
            .build_request(Method::GET, &format!("repos/{}/commits/{}", repo, sha))
            .send()?
            .error_for_status()?
            .json()?;
        Ok(commit)
    }

    fn get_pr_head_sha(&self, repo: &str, pr: i32) -> Fallible<String> {
        let pr: PullRequestData = self
            .build_request(Method::GET, &format!("repos/{}/pulls/{}", repo, pr))
            .send()?
            .error_for_status()?
            .json()?;
        Ok(pr.head.sha)
    }
}

#[derive(Deserialize)]
pub struct Error {
    pub message: String,
}

#[derive(Deserialize)]
pub struct User {
    pub id: usize,
    pub login: String,
}

#[derive(Deserialize)]
pub struct EventIssueComment {
    pub action: String,
    pub issue: Issue,
    pub comment: Comment,
    pub sender: User,
    pub repository: Repository,
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
pub struct PullRequestData {
    pub head: PullRequestHead,
}

#[derive(Deserialize)]
pub struct PullRequestHead {
    pub sha: String,
}

#[derive(Deserialize)]
pub struct Repository {
    pub full_name: String,
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

#[derive(Deserialize)]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<CommitParent>,
}

#[derive(Deserialize)]
pub struct CommitParent {
    pub sha: String,
}
