mod github;
mod http;
mod tokens;
mod webhooks;

use errors::*;
use hyper::Method;
use server::github::GitHubApi;
use server::http::Server;
use server::tokens::Tokens;

pub struct Data {
    pub bot_username: String,
    pub github: GitHubApi,
    pub tokens: Tokens,
}

pub fn run() -> Result<()> {
    let tokens = tokens::Tokens::load()?;
    let github = GitHubApi::new(&tokens);
    let bot_username = github.username()?;

    info!("bot username: {}", bot_username);

    let mut server = Server::new(Data {
        bot_username,
        github,
        tokens,
    })?;

    server.add_route(Method::Post, "/webhooks", webhooks::handle);

    info!("running server...");
    server.run()?;
    Ok(())
}
