use assets;
use chrono::SecondsFormat;
use errors::*;
use futures::{future, Future, Stream};
use hyper::header::{ContentLength, ContentType};
use hyper::server::{Request, Response};
use server::agents::AgentStatus;
use server::experiments::Status;
use server::http::Handler;
use server::http::{Context, ResponseExt, ResponseFuture};
use server::Data;
use std::sync::Arc;

pub struct StaticFile(pub &'static str);

impl Handler<Data> for StaticFile {
    fn handle(&self, _req: Request, _data: Arc<Data>, _ctx: Arc<Context>) -> ResponseFuture {
        if let Ok(asset) = assets::load(self.0) {
            if let Ok(content) = asset.content() {
                return Response::new()
                    .with_header(ContentLength(content.len() as u64))
                    .with_header(ContentType(asset.mime().clone()))
                    .with_body(content)
                    .as_future();
            }
        }

        Response::text(format!("File not found: {}", self.0)).as_future()
    }
}

#[derive(Serialize)]
struct LayoutContext {
    git_revision: Option<&'static str>,
}

impl LayoutContext {
    fn new() -> Self {
        LayoutContext {
            git_revision: ::GIT_REVISION,
        }
    }
}

#[derive(Serialize)]
struct ExperimentData {
    name: String,
    status_class: &'static str,
    status_pretty: &'static str,
    assigned_to: Option<String>,
    progress: u8,
    priority: i32,
}

#[derive(Serialize)]
struct IndexContext {
    layout: LayoutContext,
    experiments: Vec<ExperimentData>,
}

html_endpoint!(index: |_body, data| -> String {
    let mut queued = Vec::new();
    let mut running = Vec::new();
    let mut needs_report = Vec::new();
    let mut generating_report = Vec::new();
    let mut report_failed = Vec::new();

    for experiment in data.experiments.all()? {
        // Don't include completed experiments in the queue
        if experiment.server_data.status == Status::Completed {
            continue;
        }

        let (status_class, status_pretty, vector) = match experiment.server_data.status {
            Status::Queued => ("", "Queued", &mut queued),
            Status::Running => ("orange", "Running", &mut running),
            Status::NeedsReport => ("orange", "Needs report", &mut needs_report),
            Status::GeneratingReport => ("orange", "Generating report", &mut generating_report),
            Status::ReportFailed => ("red", "Report failed", &mut report_failed),
            Status::Completed => unreachable!(),
        };

        vector.push(ExperimentData {
            name: experiment.experiment.name.clone(),
            status_class,
            status_pretty,
            assigned_to: experiment.server_data.assigned_to.clone(),
            priority: experiment.server_data.priority,
            progress: experiment.progress(&data.db)?,
        });
    }

    let mut experiments = Vec::new();
    experiments.append(&mut report_failed);
    experiments.append(&mut generating_report);
    experiments.append(&mut needs_report);
    experiments.append(&mut running);
    experiments.append(&mut queued);

    assets::render_template("index.html", &IndexContext {
        layout: LayoutContext::new(),
        experiments,
    })
}, index_inner);

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
struct AgentsContext {
    layout: LayoutContext,
    agents: Vec<AgentData>,
}

html_endpoint!(agents: |_body, data| -> String {
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
            last_heartbeat: agent.last_heartbeat().map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true)),
            assigned_experiment: if show_assigned {
                agent.assigned_experiment().map(|ex| ex.experiment.name.clone())
            } else {
                None
            },
            git_revision: agent.git_revision().cloned(),
        });
    }

    assets::render_template("agents.html", &AgentsContext {
        layout: LayoutContext::new(),
        agents
    })
}, agents_inner);
