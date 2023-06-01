use crate::experiments::{Experiment, Mode, Status};
use crate::prelude::*;
use crate::server::routes::ui::{render_template, LayoutContext};
use crate::server::{Data, HttpError};
use chrono::{Duration, SecondsFormat, Utc};
use http::Response;
use hyper::Body;
use std::sync::Arc;

#[derive(Serialize)]
struct ExperimentData {
    name: String,
    status_class: &'static str,
    status_pretty: &'static str,
    mode: &'static str,
    assigned_to: Option<String>,
    requirement: Option<String>,
    progress: u8,
    priority: i32,
}

impl ExperimentData {
    fn new(data: &Data, experiment: &Experiment) -> Fallible<Self> {
        let (status_class, status_pretty, show_progress) = match experiment.status {
            Status::Queued => ("", "Queued", true),
            Status::Running => ("orange", "Running", true),
            Status::NeedsReport => ("orange", "Needs report", false),
            Status::GeneratingReport => ("orange", "Generating report", false),
            Status::ReportFailed => ("red", "Report failed", false),
            Status::Completed => ("green", "Completed", false),
        };

        Ok(ExperimentData {
            name: experiment.name.clone(),
            status_class,
            status_pretty,
            mode: match experiment.mode {
                Mode::BuildAndTest => "cargo test",
                Mode::BuildOnly => "cargo build",
                Mode::CheckOnly => "cargo check",
                Mode::Clippy => "cargo clippy",
                Mode::Rustdoc => "cargo doc",
                Mode::UnstableFeatures => "unstable features",
            },
            assigned_to: experiment.assigned_to.as_ref().map(|a| a.to_string()),
            priority: experiment.priority,
            requirement: experiment.requirement.clone(),
            progress: if show_progress {
                experiment.progress(&data.db)?
            } else {
                100
            },
        })
    }
}

#[derive(Serialize)]
struct ListContext {
    layout: LayoutContext,
    experiments: Vec<ExperimentData>,
}

pub fn endpoint_queue(data: Arc<Data>) -> Fallible<Response<Body>> {
    let mut queued = Vec::new();
    let mut running = Vec::new();
    let mut needs_report = Vec::new();
    let mut generating_report = Vec::new();
    let mut report_failed = Vec::new();

    for experiment in &Experiment::unfinished(&data.db)? {
        // Don't include completed experiments in the queue
        if experiment.status == Status::Completed {
            continue;
        }

        let ex = ExperimentData::new(&data, experiment)?;

        match experiment.status {
            Status::Queued => queued.push(ex),
            Status::Running => running.push(ex),
            Status::NeedsReport => needs_report.push(ex),
            Status::GeneratingReport => generating_report.push(ex),
            Status::ReportFailed => report_failed.push(ex),
            Status::Completed => unreachable!(),
        };
    }

    let mut experiments = Vec::new();
    experiments.append(&mut report_failed);
    experiments.append(&mut generating_report);
    experiments.append(&mut needs_report);
    experiments.append(&mut running);
    experiments.append(&mut queued);

    render_template(
        "ui/queue.html",
        &ListContext {
            layout: LayoutContext::new(),
            experiments,
        },
    )
}

#[derive(Serialize)]
struct ExperimentExt {
    #[serde(flatten)]
    common: ExperimentData,

    github_url: Option<String>,
    report_url: Option<String>,

    created_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,

    total_jobs: u32,
    completed_jobs: u32,
    duration: Option<String>,
    estimated_end: Option<String>,
    average_job_duration: Option<String>,
}

#[derive(Serialize)]
struct ExperimentContext {
    experiment: ExperimentExt,
    layout: LayoutContext,
}

fn humanize(duration: Duration) -> String {
    let duration = match duration.to_std() {
        Ok(d) => d,
        Err(_) => {
            // Don't try to make it pretty as a fallback.
            return format!("{:?}", duration);
        }
    };
    if duration.as_secs() < 60 {
        format!("{duration:?}")
    } else if duration.as_secs() < 60 * 60 {
        format!("{} minutes", duration.as_secs() / 60)
    } else {
        format!("{:.1} hours", duration.as_secs_f64() / 60.0 / 60.0)
    }
}

pub fn endpoint_experiment(name: String, data: Arc<Data>) -> Fallible<Response<Body>> {
    if let Some(ex) = Experiment::get(&data.db, &name)? {
        let (completed_jobs, total_jobs) = ex.raw_progress(&data.db)?;

        let (duration, estimated_end, average_job_duration) =
            if completed_jobs > 0 && total_jobs > 0 {
                if let Some(started_at) = ex.started_at {
                    let res = if let Some(completed_at) = ex.completed_at {
                        let total = completed_at.signed_duration_since(started_at);
                        (Some(total), None, total / completed_jobs as i32)
                    } else {
                        let total = Utc::now().signed_duration_since(started_at);
                        let job_duration = total / completed_jobs as i32;
                        (
                            None,
                            Some(job_duration * (total_jobs as i32 - completed_jobs as i32)),
                            job_duration,
                        )
                    };

                    let job_duration = humanize(res.2);

                    (res.0.map(humanize), res.1.map(humanize), Some(job_duration))
                } else {
                    (None, None, None)
                }
            } else {
                (None, None, None)
            };

        let experiment = ExperimentExt {
            common: ExperimentData::new(&data, &ex)?,

            github_url: ex.github_issue.map(|i| i.html_url),
            report_url: ex.report_url.clone(),

            created_at: ex.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            started_at: ex
                .started_at
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
            completed_at: ex
                .completed_at
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),

            total_jobs,
            completed_jobs,
            duration,
            estimated_end,
            average_job_duration,
        };

        render_template(
            "ui/experiment.html",
            &ExperimentContext {
                layout: LayoutContext::new(),
                experiment,
            },
        )
    } else {
        Err(HttpError::NotFound.into())
    }
}
