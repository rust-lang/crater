use config::Config;
use errors::*;
use hyper::header::Authorization;
use hyper::server::{Request, Response};
use server::api_types::{ApiResponse, CraterToken};
use server::github::GitHubApi;
use server::http::{Context, Handler, ResponseExt, ResponseFuture};
use server::Data;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

enum TokenType {
    Agent,
}

pub struct AuthDetails {
    pub name: String,
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
            (self.func)(req, data, ctx, AuthDetails { name: name.clone() })
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
            // Cache the list of teams in an org
            if !orgs.contains_key(org) {
                orgs.insert(org.clone(), github.list_teams(org)?);
            }

            let members = github.team_members(orgs[org][team])?;
            for member in &members {
                new_cache.insert(member.clone());
            }
        }

        // Update the shared cache
        let mut cache = self.cached_usernames.write().unwrap();
        *cache = new_cache;

        Ok(())
    }

    pub fn allowed(&self, username: &str) -> bool {
        self.cached_usernames.read().unwrap().contains(username)
    }
}
