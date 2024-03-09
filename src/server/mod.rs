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
use http::{header::HeaderValue, Response};
use hyper::Body;
use metrics::Metrics;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use warp::Filter;

lazy_static! {
    static ref SERVER_HEADER: String =
        format!("crater/{}", crate::GIT_REVISION.unwrap_or("unknown"));
}

#[derive(Debug, thiserror::Error, PartialEq, Eq, Copy, Clone)]
pub enum HttpError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
}

impl warp::reject::Reject for HttpError {}

#[derive(Clone)]
pub struct Data {
    pub config: Config,
    pub tokens: Tokens,
    pub agents: Agents,
    pub db: Database,
    pub reports_worker: reports::ReportsWorker,
    pub record_progress_worker: routes::agent::RecordProgressThread,
    pub acl: ACL,
    pub metrics: Metrics,
}

#[derive(Clone)]
pub struct GithubData {
    pub bot_username: String,
    pub api: GitHubApi,
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
                api: github,
                bot_username,
                tokens,
            })
        })
        .transpose()?;
    let agents = Agents::new(db.clone(), &tokens)?;
    info!("loaded agents...");
    let acl = ACL::new(&config, github_data.as_ref())?;
    let metrics = Metrics::new()?;
    info!("initialized metrics...");

    let data = Data {
        record_progress_worker: routes::agent::RecordProgressThread::new(
            db.clone(),
            metrics.clone(),
        ),
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
    info!("spawned reports worker...");
    cronjobs::spawn(data.clone());

    info!("running server on {}...", bind);

    let data = Arc::new(data);
    let github_data = github_data.map(Arc::new);

    let record_progress_worker = data.record_progress_worker.clone();
    let routes = warp::any()
        .and(warp::any().map(move || record_progress_worker.clone().start_request()))
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
        .map(
            |_guard: routes::agent::RequestGuard, mut resp: Response<Body>| {
                resp.headers_mut().insert(
                    http::header::SERVER,
                    HeaderValue::from_static(&SERVER_HEADER),
                );
                resp
            },
        );

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        warp::serve(routes).run(bind).await;
    });

    Ok(())
}
