use config::Config;
use errors::*;
use http::header::{HeaderMap, AUTHORIZATION, USER_AGENT};
use regex::Regex;
use server::github::GitHubApi;
use server::Data;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use warp::{self, Filter, Rejection};

lazy_static! {
    static ref GIT_REVISION_RE: Regex =
        Regex::new(r"^crater(-agent)?/(?P<sha>[a-f0-9]{7,40})$").unwrap();
}

#[derive(Copy, Clone)]
pub enum TokenType {
    Agent,
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

fn check_auth(data: &Data, headers: &HeaderMap, token_type: TokenType) -> Option<AuthDetails> {
    // Try to extract the git revision from the User-Agent header
    let git_revision = if let Some(ua_value) = headers.get(USER_AGENT) {
        if let Ok(ua) = ua_value.to_str() {
            if let Some(cap) = GIT_REVISION_RE.captures(ua) {
                Some(cap["sha"].to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    if let Some(authorization_value) = headers.get(AUTHORIZATION) {
        if let Ok(authorization) = authorization_value.to_str() {
            if let Some(token) = parse_token(authorization) {
                let tokens = match token_type {
                    TokenType::Agent => &data.tokens.agents,
                };

                if let Some(name) = tokens.get(token) {
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
    token_type: TokenType,
) -> impl Filter<Extract = (AuthDetails,), Error = Rejection> + Clone {
    warp::header::headers_cloned().and_then(move |headers| {
        match check_auth(&data, &headers, token_type) {
            Some(details) => Ok(details),
            None => Err(warp::reject::forbidden()),
        }
    })
}

#[derive(Debug, Clone)]
pub struct ACL {
    cached_usernames: Arc<RwLock<HashSet<String>>>,
    users: Vec<String>,
    teams: Vec<(String, String)>,
}

impl ACL {
    pub fn new(config: &Config, github: &GitHubApi) -> Result<Self> {
        let mut users = Vec::new();
        let mut teams = Vec::new();

        for item in &config.server.bot_acl {
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
            users,
            teams,
        };

        acl.refresh_cache(github)?;
        Ok(acl)
    }

    pub fn refresh_cache(&self, github: &GitHubApi) -> Result<()> {
        // A new HashSet is created instead of clearing the old one
        // This is done because if an error occurs the old cache is not flushed
        let mut new_cache = HashSet::new();

        for user in &self.users {
            new_cache.insert(user.clone());
        }

        let mut orgs = HashMap::new();
        for &(ref org, ref team) in &self.teams {
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
    ) -> Result<()> {
        // Cache the list of teams in an org
        if !orgs.contains_key(org) {
            orgs.insert(org.to_string(), github.list_teams(org)?);
        }

        let members = github.team_members(
            *orgs[org]
                .get(team)
                .ok_or_else(|| format!("team {}/{} doesn't exist", org, team))?,
        )?;
        for member in &members {
            new_cache.insert(member.clone());
        }

        Ok(())
    }

    pub fn allowed(&self, username: &str) -> bool {
        self.cached_usernames.read().unwrap().contains(username)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_token;

    #[test]
    fn test_parse_token() {
        assert_eq!(parse_token("foo"), None);
        assert_eq!(parse_token("foo bar"), None);
        assert_eq!(parse_token("CraterToken"), None);
        assert_eq!(parse_token("CraterToken foo"), Some("foo"));
        assert_eq!(parse_token("CraterToken foo bar"), None);
    }
}
