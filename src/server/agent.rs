use errors::*;
use ex::Experiment;
use futures::{future, Future, Stream};
use hyper::server::{Request, Response};
use serde_json;
use server::Data;
use server::api_types::{AgentConfig, ApiResponse};
use server::auth::AuthDetails;
use server::experiments::Status;
use server::http::{Context, ResponseExt, ResponseFuture};
use server::messages::{Label, Message};
use server::results::{self, TaskResult};
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
    let mut experiments = data.experiments.lock().unwrap();

    let next = experiments.next(&auth.name)?;
    if let Some((new, ex)) = next {
        if new {
            Message::new()
                .line(
                    "construction",
                    format!(
                        "Experiment **`{}`** is now **running** on agent `{}`.",
                        ex.experiment.name,
                        auth.name,
                    ),
                )
                .send(&ex.server_data.github_issue, &data)?;
        }

        Ok(ApiResponse::Success { result: Some(ex.experiment.clone()) })
    } else {
        Ok(ApiResponse::Success { result: None })
    }
}, next_ex_inner);

api_endpoint!(complete_ex: |_body, data, auth: AuthDetails| -> bool {
    let (name, github_issue) = {
        let mut experiments = data.experiments.lock().unwrap();
        let name = experiments
            .run_by_agent(&auth.name)
            .ok_or("no experiment run by this agent")?
            .to_string();
        let ex = experiments.edit_data(&name).unwrap();
        ex.server_data.status = Status::Completed;
        ex.save()?;

        (name, ex.server_data.github_issue.to_string())
    };

    info!("experiment {} completed, generating report...", name);
    let report_url = results::generate_report(&name, &data.config, &data.tokens)?;
    info!("report for the experiment {} generated successfully!", name);

    Message::new()
        .line("tada", format!("Experiment **`{}`** is completed!", name))
        .line("newspaper", format!("[Open the full report]({}).", report_url))
        .note(
            "warning",
            "If you notice any spurious failure [please add them to the \
            blacklist](https://github.com/rust-lang-nursery/crater/blob/master/config.toml)!",
        )
        .set_label(Label::ExperimentCompleted)
        .send(&github_issue, &data)?;

    Ok(ApiResponse::Success { result: true })
}, complete_ex_inner);

api_endpoint!(record_result: |body, data, auth: AuthDetails| -> bool {
    let experiments = data.experiments.lock().unwrap();
    let result: TaskResult = serde_json::from_str(&body)?;

    let name = experiments
        .run_by_agent(&auth.name)
        .ok_or("no experiment run by this agent")?
        .to_string();
    let experiment = experiments.get(&name).unwrap();

    info!(
        "receiving a result from agent {} (ex: {}, tc: {}, crate: {})",
        auth.name,
        name,
        result.toolchain.to_string(),
        result.krate
    );

    results::store(&experiment.experiment, &result)?;

    Ok(ApiResponse::Success { result: true })
}, record_result_inner);
