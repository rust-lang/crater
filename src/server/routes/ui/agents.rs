use chrono::SecondsFormat;
use errors::*;
use http::Response;
use hyper::Body;
use server::agents::AgentStatus;
use server::routes::ui::{render_template, LayoutContext};
use server::Data;
use std::sync::Arc;

#[derive(Serialize)]
struct AgentData {
    name: String,
    status_class: &'static str,
    status_pretty: &'static str,
    last_heartbeat: Option<String>,
    assigned_experiment: Option<String>,
    git_revision: Option<String>,
}

#[derive(Serialize)]
struct ListContext {
    layout: LayoutContext,
    agents: Vec<AgentData>,
}

pub fn endpoint_list(data: Arc<Data>) -> Result<Response<Body>> {
    let mut agents = Vec::new();
    for agent in &data.agents.all()? {
        let (status_class, status_pretty, show_assigned) = match agent.status() {
            AgentStatus::Working => ("orange", "Working", true),
            AgentStatus::Idle => ("green", "Online", false),
            AgentStatus::Unreachable => ("red", "Unreachable", false),
        };

        agents.push(AgentData {
            name: agent.name().to_string(),
            status_class,
            status_pretty,
            last_heartbeat: agent
                .last_heartbeat()
                .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true)),
            assigned_experiment: if show_assigned {
                agent.assigned_experiment().map(|ex| ex.name.clone())
            } else {
                None
            },
            git_revision: agent.git_revision().cloned(),
        });
    }

    render_template(
        "ui/agents.html",
        &ListContext {
            layout: LayoutContext::new(),
            agents,
        },
    )
}
