use errors::*;
use experiments::{Assignee, Experiment, Status};
use http::{Response, StatusCode};
use hyper::Body;
use results::EncodingType;
use results::{DatabaseDB, ProgressData};
use server::api_types::{AgentConfig, ApiResponse};
use server::auth::{auth_filter, AuthDetails, TokenType};
use server::messages::Message;
use server::Data;
use std::sync::Arc;
use warp::{self, Filter, Rejection};

pub fn routes(
    data: Arc<Data>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_cloned = data.clone();
    let data_filter = warp::any().map(move || data_cloned.clone());

    let config = warp::get2()
        .and(warp::path("config"))
        .and(warp::path::index())
        .and(data_filter.clone())
        .and(auth_filter(data.clone(), TokenType::Agent))
        .map(endpoint_config);

    let next_experiment = warp::get2()
        .and(warp::path("next-experiment"))
        .and(warp::path::index())
        .and(data_filter.clone())
        .and(auth_filter(data.clone(), TokenType::Agent))
        .map(endpoint_next_experiment);

    let complete_experiment = warp::post2()
        .and(warp::path("complete-experiment"))
        .and(warp::path::index())
        .and(data_filter.clone())
        .and(auth_filter(data.clone(), TokenType::Agent))
        .map(endpoint_complete_experiment);

    let record_progress = warp::post2()
        .and(warp::path("record-progress"))
        .and(warp::path::index())
        .and(warp::body::json())
        .and(data_filter.clone())
        .and(auth_filter(data.clone(), TokenType::Agent))
        .map(endpoint_record_progress);

    let heartbeat = warp::post2()
        .and(warp::path("heartbeat"))
        .and(warp::path::index())
        .and(data_filter.clone())
        .and(auth_filter(data.clone(), TokenType::Agent))
        .map(endpoint_heartbeat);

    warp::any()
        .and(
            config
                .or(next_experiment)
                .unify()
                .or(complete_experiment)
                .unify()
                .or(record_progress)
                .unify()
                .or(heartbeat)
                .unify(),
        ).map(handle_results)
        .recover(handle_errors)
        .unify()
}

fn endpoint_config(data: Arc<Data>, auth: AuthDetails) -> Result<Response<Body>> {
    Ok(ApiResponse::Success {
        result: AgentConfig {
            agent_name: auth.name,
            crater_config: data.config.clone(),
        },
    }.into_response()?)
}

fn endpoint_next_experiment(data: Arc<Data>, auth: AuthDetails) -> Result<Response<Body>> {
    let next = Experiment::next(&data.db, &Assignee::Agent(auth.name.clone()))?;

    let result = if let Some((new, mut ex)) = next {
        if new {
            if let Some(ref github_issue) = ex.github_issue {
                Message::new()
                    .line(
                        "construction",
                        format!(
                            "Experiment **`{}`** is now **running** on agent `{}`.",
                            ex.name, auth.name,
                        ),
                    ).send(&github_issue.api_url, &data)?;
            }
        }

        ex.remove_completed_crates(&data.db)?;
        Some(ex)
    } else {
        None
    };

    Ok(ApiResponse::Success { result }.into_response()?)
}

fn endpoint_complete_experiment(data: Arc<Data>, auth: AuthDetails) -> Result<Response<Body>> {
    let mut ex = Experiment::run_by(&data.db, &Assignee::Agent(auth.name.clone()))?
        .ok_or("no experiment run by this agent")?;

    ex.set_status(&data.db, Status::NeedsReport)?;
    info!("experiment {} completed, marked as needs-report", ex.name);
    data.reports_worker.wake(); // Ensure the reports worker is awake

    Ok(ApiResponse::Success { result: true }.into_response()?)
}

fn endpoint_record_progress(
    result: ProgressData,
    data: Arc<Data>,
    auth: AuthDetails,
) -> Result<Response<Body>> {
    let experiment = Experiment::run_by(&data.db, &Assignee::Agent(auth.name.clone()))?
        .ok_or("no experiment run by this agent")?;

    info!(
        "received progress on experiment {} from agent {}",
        experiment.name, auth.name,
    );

    let db = DatabaseDB::new(&data.db);
    db.store(&experiment, &result, EncodingType::Gzip)?;

    Ok(ApiResponse::Success { result: true }.into_response()?)
}

fn endpoint_heartbeat(data: Arc<Data>, auth: AuthDetails) -> Result<Response<Body>> {
    if let Some(rev) = auth.git_revision {
        data.agents.set_git_revision(&auth.name, &rev)?;
    }

    data.agents.record_heartbeat(&auth.name)?;
    Ok(ApiResponse::Success { result: true }.into_response()?)
}

fn handle_results(resp: Result<Response<Body>>) -> Response<Body> {
    match resp {
        Ok(resp) => resp,
        Err(err) => ApiResponse::internal_error(err.to_string())
            .into_response()
            .unwrap(),
    }
}

fn handle_errors(err: Rejection) -> ::std::result::Result<Response<Body>, Rejection> {
    match err.status() {
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => {
            Ok(ApiResponse::not_found().into_response().unwrap())
        }
        StatusCode::FORBIDDEN => Ok(ApiResponse::unauthorized().into_response().unwrap()),
        _ => Err(err),
    }
}
