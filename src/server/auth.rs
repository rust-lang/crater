use config::Config;
use errors::*;
use hyper::header::{Authorization, UserAgent};
use hyper::server::{Request, Response};
use regex::Regex;
use server::api_types::{ApiResponse, CraterToken};
use server::github::GitHubApi;
use server::http::{Context, Handler, ResponseExt, ResponseFuture};
use server::Data;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

lazy_static! {
    static ref GIT_REVISION_RE: Regex =
        Regex::new(r"^crater(-agent)?/(?P<sha>[a-f0-9]{7,40})$").unwrap();
}

enum TokenType {
    Agent,
}

pub struct AuthDetails {
    pub name: String,
    pub git_revision: Option<String>,
}

pub struct AuthMiddleware<F>
where
    F: Fn(Request, Arc<Data>, Arc<Context>, AuthDetails) -> ResponseFuture,
{
    func: F,
    token_type: TokenType,
}

impl<F> Handler<Data> for AuthMiddleware<F>
where
    F: Fn(Request, Arc<Data>, Arc<Context>, AuthDetails) -> ResponseFuture,
{
    fn handle(&self, req: Request, data: Arc<Data>, ctx: Arc<Context>) -> ResponseFuture {
        let git_revision = {
            let user_agent = req.headers().get::<UserAgent>();

            let mut git_revision = None;
            if let Some(ua) = user_agent {
                if let Some(cap) = GIT_REVISION_RE.captures(&ua) {
                    git_revision = Some(cap["sha"].to_string());
                }
            }

            git_revision
        };

        let provided_token = req
            .headers()
            .get::<Authorization<CraterToken>>()
            .map(|t| t.token.clone());

        let mut authorized_as = None;
        if let Some(provided_token) = provided_token {
            let tokens = match self.token_type {
                TokenType::Agent => &data.tokens.agents,
            };

            if let Some(name) = tokens.get(&provided_token) {
                authorized_as = Some(name.clone());
            }
        }

        if let Some(name) = authorized_as {
            (self.func)(
                req,
                data,
                ctx,
                AuthDetails {
                    name: name.clone(),
                    git_revision,
                },
            )
        } else {
            let resp: ApiResponse<bool> = ApiResponse::Unauthorized;
            Response::api(resp).unwrap().as_future()
        }
    }
}

pub fn auth_agent<F>(func: F) -> AuthMiddleware<F>
where
    F: Fn(Request, Arc<Data>, Arc<Context>, AuthDetails) -> ResponseFuture,
{
    AuthMiddleware {
        func,
        token_type: TokenType::Agent,
    }
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
