use errors::*;
use ex::Experiment;
use futures::Future;
use futures::future;
use hyper::StatusCode;
use hyper::server::{Request, Response};
use server::Data;
use server::auth::AuthDetails;
use server::experiments::Status;
use server::http::{Context, ResponseExt, ResponseFuture};
use std::sync::Arc;

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
pub fn config(
    _req: Request,
    _data: Arc<Data>,
    _ctx: Arc<Context>,
    auth: AuthDetails,
) -> ResponseFuture {
    Response::json(&json!({
        "agent-name": auth.name,
    })).unwrap()
        .as_future()
}

fn get_next_experiment(data: &Data, auth: &AuthDetails) -> Result<Option<Experiment>> {
    let mut experiments = data.experiments.lock().unwrap();

    let next = experiments.next(&auth.name)?;
    if let Some((new, ex)) = next {
        if new {
            data.github.post_comment(
                &ex.server_data.github_issue,
                &format!(
                    ":construction: Experiment **`{}`** is now **running** \
                     on agent `{}` :construction:",
                    ex.experiment.name, auth.name,
                ),
            )?;
        }

        Ok(Some(ex.experiment.clone()))
    } else {
        Ok(None)
    }
}

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
pub fn next_ex(
    _req: Request,
    data: Arc<Data>,
    ctx: Arc<Context>,
    auth: AuthDetails,
) -> ResponseFuture {
    Box::new(
        ctx.pool
            .spawn_fn(move || future::done(get_next_experiment(&data, &auth)))
            .and_then(|data| future::ok(Response::json(&data).unwrap()))
            .or_else(|err| {
                error!("internal error: {}", err);
                Response::json(&json!({
                    "error": err.to_string(),
                })).unwrap()
                    .with_status(StatusCode::InternalServerError)
                    .as_future()
            }),
    )
}

fn complete_experiment(data: &Data, auth: &AuthDetails) -> Result<()> {
    let mut experiments = data.experiments.lock().unwrap();

    let name = experiments
        .run_by_agent(&auth.name)
        .ok_or("no experiment run by this agent")?
        .to_string();
    let ex = experiments.edit_data(&name).unwrap();
    ex.server_data.status = Status::Completed;
    ex.save()?;

    data.github.post_comment(
        &ex.server_data.github_issue,
        &format!(":tada: Experiment **`{}`** completed!", name),
    )?;

    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
pub fn complete_ex(
    _req: Request,
    data: Arc<Data>,
    ctx: Arc<Context>,
    auth: AuthDetails,
) -> ResponseFuture {
    Box::new(
        ctx.pool
            .spawn_fn(move || future::done(complete_experiment(&data, &auth)))
            .and_then(|_| future::ok(Response::text("OK\n")))
            .or_else(|err| {
                error!("internal error: {}", err);
                Response::json(&json!({
                    "error": err.to_string(),
                })).unwrap()
                    .with_status(StatusCode::InternalServerError)
                    .as_future()
            }),
    )
}
