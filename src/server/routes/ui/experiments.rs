use errors::*;
use ex::ExMode;
use http::Response;
use hyper::Body;
use server::experiments::Status;
use server::routes::ui::{render_template, LayoutContext};
use server::Data;
use std::sync::Arc;

#[derive(Serialize)]
struct ExperimentData {
    name: String,
    status_class: &'static str,
    status_pretty: &'static str,
    mode: &'static str,
    assigned_to: Option<String>,
    progress: u8,
    priority: i32,
}

#[derive(Serialize)]
struct ListContext {
    layout: LayoutContext,
    experiments: Vec<ExperimentData>,
}

pub fn endpoint_queue(data: Arc<Data>) -> Result<Response<Body>> {
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
            mode: match experiment.experiment.mode {
                ExMode::BuildAndTest => "cargo test",
                ExMode::BuildOnly => "cargo build",
                ExMode::CheckOnly => "cargo check",
                ExMode::UnstableFeatures => "unstable features",
            },
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

    render_template(
        "queue.html",
        &ListContext {
            layout: LayoutContext::new(),
            experiments,
        },
    )
}
