pub mod agents;
pub mod api_types;
mod auth;
mod github;
mod messages;
mod reports;
mod routes;
pub mod tokens;

use crate::config::Config;
use crate::db::Database;
use crate::prelude::*;
use crate::server::agents::Agents;
use crate::server::auth::ACL;
use crate::server::github::GitHubApi;
use crate::server::tokens::Tokens;
use http::{self, header::HeaderValue, Response};
use hyper::Body;
use std::sync::Arc;
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
    pub bot_username: String,
    pub config: Config,
    pub github: GitHubApi,
    pub tokens: Tokens,
    pub agents: Agents,
    pub db: Database,
    pub reports_worker: reports::ReportsWorker,
    pub acl: ACL,
}

pub fn run(config: Config) -> Fallible<()> {
    let db = Database::open()?;
    let tokens = tokens::Tokens::load()?;
    let github = GitHubApi::new(&tokens);
    let agents = Agents::new(db.clone(), &tokens)?;
    let bot_username = github.username()?;
    let acl = ACL::new(&config, &github)?;

    info!("bot username: {}", bot_username);

    let data = Data {
        bot_username,
        config,
        github,
        tokens,
        agents,
        db: db.clone(),
        reports_worker: reports::ReportsWorker::new(),
        acl,
    };

    data.reports_worker.spawn(data.clone());

    info!("running server...");

    let data = Arc::new(data);

    let routes = warp::any()
        .and(
            warp::any()
                .and(warp::path("webhooks").and(routes::webhooks::routes(data.clone())))
                .or(warp::path("agent-api").and(routes::agent::routes(data.clone())))
                .unify()
                .or(routes::ui::routes(data.clone()))
                .unify(),
        )
        .map(|mut resp: Response<Body>| {
            resp.headers_mut().insert(
                http::header::SERVER,
                HeaderValue::from_static(&SERVER_HEADER),
            );
            resp
        });

    warp::serve(routes).run(([127, 0, 0, 1], 8000));

    Ok(())
}
