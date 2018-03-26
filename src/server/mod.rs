mod agent;
mod auth;
mod github;
mod http;
mod tokens;
mod webhooks;
mod experiments;

use config::Config;
use errors::*;
use hyper::Method;
use server::auth::auth_agent;
use server::experiments::Experiments;
use server::github::GitHubApi;
use server::http::Server;
use server::tokens::Tokens;
use std::sync::{Arc, Mutex};

pub struct Data {
    pub bot_username: String,
    pub config: Config,
    pub github: GitHubApi,
    pub tokens: Tokens,
    pub experiments: Arc<Mutex<Experiments>>,
}

pub fn run(config: Config) -> Result<()> {
    let tokens = tokens::Tokens::load()?;
    let github = GitHubApi::new(&tokens);
    let bot_username = github.username()?;

    info!("bot username: {}", bot_username);

    let mut server = Server::new(Data {
        bot_username,
        config,
        github,
        tokens,
        experiments: Arc::new(Mutex::new(Experiments::new()?)),
    })?;

    server.add_route(Method::Get, "/agent-api/config", auth_agent(agent::config));
    server.add_route(
        Method::Get,
        "/agent-api/next-experiment",
        auth_agent(agent::next_ex),
    );
    server.add_route(
        Method::Post,
        "/agent-api/complete-experiment",
        auth_agent(agent::complete_ex),
    );

    server.add_route(Method::Post, "/webhooks", webhooks::handle);

    info!("running server...");
    server.run()?;
    Ok(())
}
