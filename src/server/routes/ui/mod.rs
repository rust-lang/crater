use assets;
use errors::*;
use http::header::{HeaderValue, CONTENT_TYPE};
use http::{Response, StatusCode};
use hyper::Body;
use serde::Serialize;
use server::Data;
use std::sync::Arc;
use warp::{self, Filter, Rejection};

mod agents;
mod experiments;

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

pub fn routes(
    data: Arc<Data>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_filter = warp::any().map(move || data.clone());

    let index = warp::get2()
        .and(warp::path::index())
        .and(data_filter.clone())
        .map(experiments::endpoint_queue);

    let agents = warp::get2()
        .and(warp::path("agents"))
        .and(warp::path::index())
        .and(data_filter.clone())
        .map(agents::endpoint_list);

    let assets = warp::get2()
        .and(warp::path("assets"))
        .and(warp::path::param())
        .and(warp::path::index())
        .map(endpoint_assets);

    warp::any()
        .and(index.or(agents).unify().or(assets).unify())
        .map(handle_results)
        .recover(handle_errors)
        .unify()
}

fn endpoint_assets(path: String) -> Result<Response<Body>> {
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

fn error_404() -> Result<Response<Body>> {
    let mut resp = render_template(
        "404.html",
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
        "500.html",
        &ErrorContext {
            layout: LayoutContext::new(),
        },
    ) {
        Ok(resp) => resp,
        Err(err) => {
            error!("failed to render 500 error page!");
            ::util::report_error(&err);
            Response::new("500: Internal Server Error\n".into())
        }
    };

    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    resp
}

fn handle_results(resp: Result<Response<Body>>) -> Response<Body> {
    match resp {
        Ok(resp) => resp,
        Err(err) => {
            ::util::report_error(&err);
            error_500()
        }
    }
}

fn handle_errors(err: Rejection) -> ::std::result::Result<Response<Body>, Rejection> {
    match err.status() {
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => match error_404() {
            Ok(resp) => Ok(resp),
            Err(err) => {
                error!("failed to render 404 page!");
                ::util::report_error(&err);
                Ok(error_500())
            }
        },
        _ => Err(err),
    }
}

fn render_template<C: Serialize>(name: &str, context: &C) -> Result<Response<Body>> {
    let mut resp = Response::new(assets::render_template(name, context)?.into());
    resp.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
    Ok(resp)
}
