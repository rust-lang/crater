use errors::*;
use ex::Experiment;
use futures::{future, Future, Stream};
use hyper::server::{Request, Response};
use serde_json;
use server::api_types::{AgentConfig, ApiResponse};
use server::auth::AuthDetails;
use server::experiments::Status;
use server::http::{Context, ResponseExt, ResponseFuture};
use server::messages::Message;
use server::results::{ProgressData, ResultsDB};
use server::Data;
use std::sync::Arc;

api_endpoint!(config: |_body, data, auth: AuthDetails| -> AgentConfig {
    Ok(ApiResponse::Success {
        result: AgentConfig {
            agent_name: auth.name,
            crater_config: data.config.clone(),
        },
    })
}, config_inner);

api_endpoint!(next_ex: |_body, data, auth: AuthDetails| -> Option<Experiment> {
    let next = data.experiments.next(&auth.name)?;
    if let Some((new, mut ex)) = next {
        if new {
            if let Some(ref github_issue) = ex.server_data.github_issue {
                Message::new()
                    .line(
                        "construction",
                        format!(
                            "Experiment **`{}`** is now **running** on agent `{}`.",
                            ex.experiment.name,
                            auth.name,
                        ),
                    )
                    .send(&github_issue.api_url, &data)?;
            }
        }

        ex.remove_completed_crates(&data.db)?;
        Ok(ApiResponse::Success { result: Some(ex.experiment) })
    } else {
        Ok(ApiResponse::Success { result: None })
    }
}, next_ex_inner);

api_endpoint!(complete_ex: |_body, data, auth: AuthDetails| -> bool {
    let mut ex = data.experiments
        .run_by_agent(&auth.name)?
        .ok_or("no experiment run by this agent")?;

    ex.set_status(&data.db, Status::NeedsReport)?;
    info!("experiment {} completed, marked as needs-report", ex.experiment.name);
    data.reports_worker.wake(); // Ensure the reports worker is awake

    Ok(ApiResponse::Success { result: true })
}, complete_ex_inner);

api_endpoint!(record_progress: |body, data, auth: AuthDetails| -> bool {
    let result: ProgressData = serde_json::from_slice(&body)?;

    let experiment = data.experiments
        .run_by_agent(&auth.name)?
        .ok_or("no experiment run by this agent")?;

    info!(
        "received progress on experiment {} from agent {}",
        experiment.experiment.name,
        auth.name,
    );

    let db = ResultsDB::new(&data.db);
    db.store(&experiment.experiment, &result)?;

    Ok(ApiResponse::Success { result: true })
}, record_progress_inner);

api_endpoint!(heartbeat: |_body, data, auth: AuthDetails| -> bool {
    if let Some(rev) = auth.git_revision {
        data.agents.set_git_revision(&auth.name, &rev)?;
    }

    data.agents.record_heartbeat(&auth.name)?;
    Ok(ApiResponse::Success { result: true })
}, heartbeat_inner);
