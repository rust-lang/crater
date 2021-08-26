pub mod agents;
pub mod api_types;
mod auth;
mod cronjobs;
mod github;
mod messages;
mod metrics;
mod reports;
mod routes;
pub mod tokens;
mod try_builds;

use crate::config::Config;
use crate::db::Database;
use crate::prelude::*;
use crate::server::agents::Agents;
use crate::server::auth::ACL;
use crate::server::github::{GitHub, GitHubApi};
use crate::server::tokens::{BotTokens, Tokens};
use http::{self, header::HeaderValue, Response};
use hyper::Body;
use metrics::Metrics;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use warp::{self, Filter};

lazy_static! {
    static ref SERVER_HEADER: String =
        format!("crater/{}", crate::GIT_REVISION.unwrap_or("unknown"));
}

#[derive(Debug, Fail, PartialEq, Eq, Copy, Clone)]
pub enum HttpError {
    #[fail(display = "not found")]
    NotFound,
    #[fail(display = "forbidden")]
    Forbidden,
}

#[derive(Clone)]
pub struct Data {
    pub config: Config,
    pub tokens: Tokens,
    pub agents: Agents,
    pub db: Database,
    pub reports_worker: reports::ReportsWorker,
    pub acl: ACL,
    pub metrics: Metrics,
}

#[derive(Clone)]
pub struct GithubData {
    pub bot_username: String,
    pub github: GitHubApi,
    pub tokens: BotTokens,
}

pub fn run(config: Config, bind: SocketAddr) -> Fallible<()> {
    let db = Database::open()?;
    let tokens = tokens::Tokens::load()?;
    let github_data = tokens
        .bot
        .as_ref()
        .cloned()
        .map(|tokens| {
            let github = GitHubApi::new(&tokens);
            let bot_username = github.username()?;
            info!("bot username: {}", bot_username);
            Fallible::Ok(GithubData {
                github,
                bot_username,
                tokens,
            })
        })
        .transpose()?;
    let agents = Agents::new(db.clone(), &tokens)?;
    let acl = ACL::new(&config, github_data.as_ref())?;
    let metrics = Metrics::new()?;

    let data = Data {
        config,
        tokens,
        agents,
        db,
        reports_worker: reports::ReportsWorker::new(),
        acl,
        metrics,
    };

    let mutex = Arc::new(Mutex::new(data.clone()));

    data.reports_worker.spawn(data.clone(), github_data.clone());
    cronjobs::spawn(data.clone());

    info!("running server on {}...", bind);

    let data = Arc::new(data);
    let github_data = github_data.map(Arc::new);

    let routes = warp::any()
        .and(
            warp::any()
                .and(
                    warp::path("webhooks")
                        .and(routes::webhooks::routes(data.clone(), github_data.clone())),
                )
                .or(warp::path("agent-api").and(routes::agent::routes(
                    data.clone(),
                    mutex,
                    github_data,
                )))
                .unify()
                .or(warp::path("metrics").and(routes::metrics::routes(data.clone())))
                .unify()
                .or(routes::ui::routes(data))
                .unify(),
        )
        .map(|mut resp: Response<Body>| {
            resp.headers_mut().insert(
                http::header::SERVER,
                HeaderValue::from_static(&SERVER_HEADER),
            );
            resp
        });

    warp::serve(routes).run(bind);

    Ok(())
}
