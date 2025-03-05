use crate::config::Config;
use crate::prelude::*;
use crate::server::github::{GitHub, GitHubApi};
use crate::server::{Data, GithubData, HttpError};
use http::header::{HeaderMap, AUTHORIZATION, USER_AGENT};
use regex::Regex;
use rust_team_data::v1 as team_data;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use warp::{Filter, Rejection};

lazy_static! {
    static ref GIT_REVISION_RE: Regex =
        Regex::new(r"^crater(-agent)?/(?P<sha>[a-f0-9]{7,40})( \(.*\))?$").unwrap();
}

pub struct AuthDetails {
    pub name: String,
    pub git_revision: Option<String>,
}

fn parse_token(authorization: &str) -> Option<&str> {
    let mut segments = authorization.split(' ');
    if let Some(scope) = segments.next() {
        if scope == "CraterToken" {
            if let Some(token) = segments.next() {
                if segments.next().is_none() {
                    return Some(token);
                }
            }
        }
    }

    None
}

fn git_revision(user_agent: &str) -> Option<String> {
    GIT_REVISION_RE
        .captures(user_agent)
        .map(|cap| cap["sha"].to_string())
}

fn check_auth(data: &Data, headers: &HeaderMap) -> Option<AuthDetails> {
    // Try to extract the git revision from the User-Agent header
    let git_revision = if let Some(ua_value) = headers.get(USER_AGENT) {
        if let Ok(ua) = ua_value.to_str() {
            git_revision(ua)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(authorization_value) = headers.get(AUTHORIZATION) {
        if let Ok(authorization) = authorization_value.to_str() {
            if let Some(token) = parse_token(authorization) {
                if let Some(name) = data.tokens.agents.get(token) {
                    return Some(AuthDetails {
                        name: name.clone(),
                        git_revision,
                    });
                }
            }
        }
    }

    None
}

pub fn auth_filter(
    data: Arc<Data>,
) -> impl Filter<Extract = (AuthDetails,), Error = Rejection> + Clone {
    warp::header::headers_cloned().and_then(move |headers| {
        let data = data.clone();
        async move {
            match check_auth(&data, &headers) {
                Some(details) => Ok(details),
                None => Err(warp::reject::custom(HttpError::Forbidden)),
            }
        }
    })
}

#[derive(Debug, Clone)]
pub struct ACL {
    cached_usernames: Arc<RwLock<HashSet<String>>>,
    rust_teams: bool,
    users: Vec<String>,
    teams: Vec<(String, String)>,
}

impl ACL {
    pub fn new(config: &Config, github: Option<&GithubData>) -> Fallible<Self> {
        let mut users = Vec::new();
        let mut teams = Vec::new();

        for item in &config.server.bot_acl.github {
            if let Some(middle) = item.find('/') {
                let org = item[..middle].to_string();
                let team = item[middle + 1..].to_string();
                teams.push((org, team));
            } else {
                users.push(item.clone());
            }
        }

        let acl = ACL {
            cached_usernames: Arc::new(RwLock::new(HashSet::new())),
            rust_teams: config.server.bot_acl.rust_teams,
            users,
            teams,
        };

        if let Some(github) = github {
            acl.refresh_cache(&github.api)?;
        }
        Ok(acl)
    }

    pub fn refresh_cache(&self, github: &GitHubApi) -> Fallible<()> {
        // A new HashSet is created instead of clearing the old one
        // This is done because if an error occurs the old cache is not flushed
        let mut new_cache = HashSet::new();

        for user in &self.users {
            new_cache.insert(user.clone());
        }

        let mut orgs = HashMap::new();
        for (org, team) in &self.teams {
            if let Err(err) = self.load_team(github, &mut new_cache, &mut orgs, org, team) {
                warn!(
                    "failed to authorize members of {}/{} to use the bot",
                    org, team
                );
                warn!("caused by: {}", err);
            }
        }

        // Update the shared cache
        let mut cache = self.cached_usernames.write().unwrap();
        *cache = new_cache;

        Ok(())
    }

    fn load_team(
        &self,
        github: &GitHubApi,
        new_cache: &mut HashSet<String>,
        orgs: &mut HashMap<String, HashMap<String, usize>>,
        org: &str,
        team: &str,
    ) -> Fallible<()> {
        // Cache the list of teams in an org
        if !orgs.contains_key(org) {
            orgs.insert(org.to_string(), github.list_teams(org)?);
        }

        let members = github.team_members(
            *orgs[org]
                .get(team)
                .ok_or_else(|| anyhow!("team {org}/{team} doesn't exist"))?,
        )?;
        for member in &members {
            new_cache.insert(member.clone());
        }

        Ok(())
    }

    pub fn allowed(&self, username: &str, user_id: u64) -> Fallible<bool> {
        if self.rust_teams {
            let url = format!("{}/permissions/crater.json", team_data::BASE_URL);
            let members: team_data::Permission = crate::utils::http::get_sync(&url)?.json()?;
            if members.github_ids.contains(&user_id) {
                return Ok(true);
            }
        }
        Ok(self.cached_usernames.read().unwrap().contains(username))
    }
}

#[cfg(test)]
mod tests {
    use super::{git_revision, parse_token};

    #[test]
    fn test_parse_token() {
        assert_eq!(parse_token("foo"), None);
        assert_eq!(parse_token("foo bar"), None);
        assert_eq!(parse_token("CraterToken"), None);
        assert_eq!(parse_token("CraterToken foo"), Some("foo"));
        assert_eq!(parse_token("CraterToken foo bar"), None);
    }

    #[test]
    fn test_git_revision() {
        for sha in &["0000000", "0000000000000000000000000000000000000000"] {
            assert_eq!(
                git_revision(&format!("crater/{sha}")),
                Some(sha.to_string())
            );
            assert_eq!(
                git_revision(&format!("crater/{sha} (foo bar!)")),
                Some(sha.to_string())
            );
        }

        // Test with too few and too many digits
        assert!(git_revision("crater/000000").is_none());
        assert!(git_revision("crater/00000000000000000000000000000000000000000").is_none());

        // Test invalid syntaxes
        assert!(git_revision("crater/ggggggg").is_none());
        assert!(git_revision("crater/0000000(foo bar!)").is_none());
        assert!(git_revision("crater/0000000 (foo bar!) ").is_none());
        assert!(git_revision("crate/0000000").is_none());
    }
}
