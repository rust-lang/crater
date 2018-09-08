use chrono::{Duration, SecondsFormat, Utc};
use chrono_humanize::{Accuracy, HumanTime, Tense};
use errors::*;
use ex::ExMode;
use experiments::{ExperimentData as Experiment, Status};
use http::Response;
use hyper::Body;
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

impl ExperimentData {
    fn new(data: &Data, experiment: &::experiments::ExperimentData) -> Result<Self> {
        let (status_class, status_pretty) = match experiment.server_data.status {
            Status::Queued => ("", "Queued"),
            Status::Running => ("orange", "Running"),
            Status::NeedsReport => ("orange", "Needs report"),
            Status::GeneratingReport => ("orange", "Generating report"),
            Status::ReportFailed => ("red", "Report failed"),
            Status::Completed => ("green", "Completed"),
        };

        Ok(ExperimentData {
            name: experiment.experiment.name.clone(),
            status_class,
            status_pretty,
            mode: match experiment.experiment.mode {
                ExMode::BuildAndTest => "cargo test",
                ExMode::BuildOnly => "cargo build",
                ExMode::CheckOnly => "cargo check",
                ExMode::UnstableFeatures => "unstable features",
            },
            assigned_to: experiment
                .server_data
                .assigned_to
                .as_ref()
                .map(|a| a.to_string()),
            priority: experiment.server_data.priority,
            progress: experiment.progress(&data.db)?,
        })
    }
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

        let ex = ExperimentData::new(&data, &experiment)?;

        match experiment.server_data.status {
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
        "queue.html",
        &ListContext {
            layout: LayoutContext::new(),
            experiments,
        },
    )
}

#[derive(Serialize)]
struct ExperimentDataExt {
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
    experiment: ExperimentDataExt,
    layout: LayoutContext,
}

pub fn endpoint_experiment(name: String, data: Arc<Data>) -> Result<Response<Body>> {
    if let Some(ex) = Experiment::get(&data.db, &name)? {
        let (completed_jobs, total_jobs) = ex.raw_progress(&data.db)?;

        let (duration, estimated_end, average_job_duration) = if completed_jobs > 0
            && total_jobs > 0
        {
            if let Some(started_at) = ex.server_data.started_at {
                let res = if let Some(completed_at) = ex.server_data.completed_at {
                    let total = completed_at.signed_duration_since(started_at);
                    (
                        Some(total),
                        None,
                        Some((total / completed_jobs as i32).num_seconds()),
                    )
                } else {
                    let total = Utc::now().signed_duration_since(started_at);
                    let job_duration = total / completed_jobs as i32;
                    (
                        None,
                        Some(job_duration * (total_jobs as i32 - completed_jobs as i32)),
                        Some(job_duration.num_seconds()),
                    )
                };

                (
                    res.0
                        .map(|r| HumanTime::from(r).to_text_en(Accuracy::Rough, Tense::Present)),
                    res.1
                        .map(|r| HumanTime::from(r).to_text_en(Accuracy::Rough, Tense::Present)),
                    res.2.map(|r| {
                        HumanTime::from(Duration::seconds(r))
                            .to_text_en(Accuracy::Precise, Tense::Present)
                    }),
                )
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };

        let experiment = ExperimentDataExt {
            common: ExperimentData::new(&data, &ex)?,

            github_url: ex.server_data.github_issue.map(|i| i.html_url.clone()),
            report_url: ex.server_data.report_url.clone(),

            created_at: ex
                .server_data
                .created_at
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            started_at: ex
                .server_data
                .started_at
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
            completed_at: ex
                .server_data
                .completed_at
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),

            total_jobs,
            completed_jobs,
            duration,
            estimated_end,
            average_job_duration,
        };

        render_template(
            "experiment.html",
            &ExperimentContext {
                layout: LayoutContext::new(),
                experiment,
            },
        )
    } else {
        Err(ErrorKind::Error404.into())
    }
}
