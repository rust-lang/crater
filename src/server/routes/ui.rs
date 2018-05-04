use errors::*;
use file;
use futures::future::{self, Future};
use handlebars::Handlebars;
use hyper::StatusCode;
use hyper::header::{CacheControl, CacheDirective, ContentLength, ContentType};
use hyper::server::{Request, Response};
use mime;
use mime::Mime;
use server::Data;
use server::agents::AgentStatus;
use server::experiments::{Experiments, Status};
use server::http::{Context, Handler, ResponseExt, ResponseFuture};
use server::results;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

macro_rules! load_static_files {
    ($($path:expr,)*) => {{
        let mut files = HashMap::new();
        $(
            #[cfg(debug_assertions)]
            {
                warn!("loaded dynamic asset (use release builds to statically bundle it): {}", $path);
                files.insert($path, FileContent::Dynamic($path.into()));
            }

            #[cfg(not(debug_assertions))]
            {
                files.insert($path, FileContent::Static(include_str!(concat!("../../", $path))));
            }
        )*
        files
    }};
}

lazy_static! {
    static ref STATIC_FILES: HashMap<&'static str, FileContent> = load_static_files![
        "template/queue.html",
        "static/queue.css",
    ];

    static ref QUEUE_TEMPLATE: IncludedFile = static_file("template/queue.html", mime::TEXT_HTML);
}

pub enum FileContent {
    #[cfg_attr(debug_assertions, allow(dead_code))]
    Static(&'static str),
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Dynamic(PathBuf),
}

pub struct IncludedFile {
    name: &'static str,
    mime: Mime,
}

impl IncludedFile {
    pub fn load(&self, ctx: &Context) -> Box<Future<Item = String, Error = Error>> {
        match STATIC_FILES[self.name] {
            FileContent::Static(content) => Box::new(future::ok(content.to_string())),
            FileContent::Dynamic(ref path) => {
                Box::new(ctx.pool.spawn_fn(move || match file::read_string(path) {
                    Ok(content) => future::ok(content),
                    Err(e) => future::err(e),
                }))
            }
        }
    }
}

impl<D: 'static> Handler<D> for IncludedFile {
    fn handle(&self, _req: Request, _data: Arc<D>, ctx: Arc<Context>) -> ResponseFuture {
        let mime = self.mime.clone();
        Box::new(
            self.load(&ctx)
                .and_then(move |content| {
                    future::ok(
                        Response::new()
                            .with_header(ContentLength(content.len() as u64))
                            .with_header(ContentType(mime))
                            .with_header(CacheControl(vec![CacheDirective::MaxAge(60 * 60 * 24)]))
                            .with_body(content),
                    )
                })
                .or_else(|error| {
                    future::ok(
                        Response::text(format!("Internal error: {}", error))
                            .with_status(StatusCode::InternalServerError),
                    )
                }),
        )
    }
}

pub fn static_file(name: &'static str, mime: Mime) -> IncludedFile {
    IncludedFile { name, mime }
}

#[derive(Serialize)]
struct QueueExperiment {
    name: String,
    status: String,
    status_text: String,
    priority: i32,
    github_issue: Option<i32>,
    github_url: Option<String>,
    assigned_agent: String,
    report_url: Option<String>,
}

#[derive(Serialize)]
struct AgentData {
    name: String,
    status: String,
    status_text: String,
    ex_name: Option<String>,
    ex_github_issue: Option<i32>,
    ex_github_url: Option<String>,
    ex_status: Option<String>,
    ex_status_text: Option<String>,
    ex_progress: Option<String>,
}

#[derive(Serialize)]
struct QueueData {
    experiments: Vec<QueueExperiment>,
    agents: Vec<AgentData>,
    completed: Vec<QueueExperiment>,
}

fn render_queue(template: &str, data: &Data) -> Result<String> {
    let ex_db = Experiments::new(data.db.clone());

    let mut queued = Vec::new();
    let mut completed = Vec::new();

    for ex in ex_db.get_all()? {
        let new = QueueExperiment {
            name: ex.experiment.name.clone(),
            status: ex.server_data.status.to_string(),
            status_text: ex.server_data.status.pretty().to_string(),
            priority: ex.server_data.priority,
            github_issue: ex.server_data
                .github_issue
                .as_ref()
                .map(|issue| issue.number),
            github_url: ex.server_data
                .github_issue
                .as_ref()
                .map(|issue| issue.html_url.clone()),
            assigned_agent: ex.server_data.assigned_to.unwrap_or_else(|| "-".into()),
            report_url: if let Status::Completed = ex.server_data.status {
                Some(results::report_url(&ex.experiment.name, &data.tokens))
            } else {
                None
            },
        };

        match ex.server_data.status {
            Status::Queued => queued.push(new),
            Status::Completed => completed.push(new),
            _ => {}
        }
    }

    let mut agents = Vec::new();
    for agent in &data.agents.all()? {
        let status = agent.status();

        let mut ex_progress = None;
        if let Some(ex) = agent.experiment() {
            ex_progress = ex.progress(&data.db)?
                .and_then(|p| Some(format!("{:.2}%", p.executed * 100 / p.total)));
        }

        agents.push(AgentData {
            name: agent.name().to_string(),
            status: status.to_string(),
            status_text: match status {
                AgentStatus::Working => "Working",
                AgentStatus::Idle => "Idle",
                AgentStatus::Unreachable => "Unreachable",
            }.to_string(),
            ex_name: agent.experiment().map(|ex| ex.experiment.name.clone()),
            ex_status: agent
                .experiment()
                .map(|ex| ex.server_data.status.to_string()),
            ex_status_text: agent
                .experiment()
                .map(|ex| ex.server_data.status.pretty().to_string()),
            ex_github_issue: agent.experiment().and_then(|ex| {
                ex.server_data
                    .github_issue
                    .as_ref()
                    .map(|issue| issue.number)
            }),
            ex_github_url: agent.experiment().and_then(|ex| {
                ex.server_data
                    .github_issue
                    .as_ref()
                    .map(|issue| issue.html_url.clone())
            }),
            ex_progress,
        });
    }

    let data = QueueData {
        experiments: queued,
        completed,
        agents,
    };

    let hb = Handlebars::new();
    Ok(hb.template_render(template, &data)?)
}

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
pub fn queue(_req: Request, data: Arc<Data>, ctx: Arc<Context>) -> ResponseFuture {
    Box::new(
        QUEUE_TEMPLATE
            .load(&ctx)
            .and_then(move |template| {
                ctx.pool
                    .spawn_fn(move || future::done(render_queue(&template, &data)))
            })
            .and_then(|content| future::ok(Response::html(content)))
            .or_else(|error| {
                future::ok(
                    Response::text(format!("Internal error: {}", error))
                        .with_status(StatusCode::InternalServerError),
                )
            }),
    )
}
