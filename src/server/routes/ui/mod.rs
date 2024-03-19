use crate::assets;
use crate::prelude::*;
use crate::server::{Data, HttpError};
use http::header::{HeaderValue, CONTENT_TYPE};
use http::{Response, StatusCode};
use hyper::Body;
use serde::Serialize;
use std::sync::Arc;
use warp::{Filter, Rejection};

mod agents;
mod experiments;

#[derive(Serialize)]
struct LayoutContext {
    git_revision: Option<&'static str>,
}

impl LayoutContext {
    fn new() -> Self {
        LayoutContext {
            git_revision: crate::GIT_REVISION,
        }
    }
}

pub fn routes(
    data: Arc<Data>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_filter = warp::any().map(move || data.clone());

    let queue = warp::get()
        .and(warp::path::end())
        .and(data_filter.clone())
        .map(experiments::endpoint_queue);

    let experiment = warp::get()
        .and(warp::path("ex"))
        .and(warp::path::param())
        .and(warp::path::end())
        .and(data_filter.clone())
        .map(experiments::endpoint_experiment);

    let agents = warp::get()
        .and(warp::path("agents"))
        .and(warp::path::end())
        .and(data_filter)
        .map(agents::endpoint_list);

    let assets = warp::get()
        .and(warp::path("assets"))
        .and(warp::path::param())
        .and(warp::path::end())
        .map(endpoint_assets);

    warp::any()
        .and(
            queue
                .or(experiment)
                .unify()
                .or(agents)
                .unify()
                .or(assets)
                .unify(),
        )
        .map(handle_results)
        .recover(handle_errors)
        .unify()
}

fn endpoint_assets(path: String) -> Fallible<Response<Body>> {
    if let Ok(asset) = assets::load(&path) {
        if let Ok(content) = asset.content() {
            let mut resp = Response::new(content.into_owned().into());
            resp.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_str(asset.mime().as_ref()).unwrap(),
            );
            return Ok(resp);
        }
    }

    error_404()
}

#[derive(Serialize)]
struct ErrorContext {
    layout: LayoutContext,
}

fn error_404() -> Fallible<Response<Body>> {
    let mut resp = render_template(
        "ui/404.html",
        &ErrorContext {
            layout: LayoutContext::new(),
        },
    )?;

    *resp.status_mut() = StatusCode::NOT_FOUND;
    Ok(resp)
}

fn error_500() -> Response<Body> {
    // Ensure the 500 error page always renders
    let mut resp = match render_template(
        "ui/500.html",
        &ErrorContext {
            layout: LayoutContext::new(),
        },
    ) {
        Ok(resp) => resp,
        Err(err) => {
            error!("failed to render 500 error page!");
            crate::utils::report_failure(&err);
            Response::new("500: Internal Server Error\n".into())
        }
    };

    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    resp
}

fn handle_results(resp: Fallible<Response<Body>>) -> Response<Body> {
    match resp {
        Ok(resp) => resp,
        Err(err) => {
            if err
                .downcast_ref::<HttpError>()
                .map(|e| e == &HttpError::NotFound)
                .unwrap_or(false)
            {
                match error_404() {
                    Ok(content) => return content,
                    Err(err404) => {
                        crate::utils::report_failure(&err404);
                        return error_500();
                    }
                }
            }

            crate::utils::report_failure(&err);
            error_500()
        }
    }
}

async fn handle_errors(err: Rejection) -> Result<Response<Body>, Rejection> {
    if err.is_not_found() {
        match error_404() {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                error!("failed to render 404 page!");
                crate::utils::report_failure(&err);
                return Ok(error_500());
            }
        }
    }

    Err(err)
}

fn render_template<C: Serialize>(name: &str, context: &C) -> Fallible<Response<Body>> {
    let mut resp = Response::new(assets::render_template(name, context)?.into());
    resp.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
    Ok(resp)
}
