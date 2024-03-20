use crate::prelude::*;
use crate::server::agents::Agent;
use crate::server::Data;
use http::{Response, StatusCode};
use hyper::Body;
use prometheus::{Encoder, TextEncoder};
use std::sync::Arc;
use warp::{Filter, Rejection};

pub fn routes(
    data: Arc<Data>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_filter = warp::any().map(move || data.clone());

    warp::get()
        .and(warp::path::end())
        .and(data_filter)
        .map(|data| match endpoint_metrics(data) {
            Ok(resp) => resp,
            Err(err) => {
                error!("error while processing metrics");
                crate::utils::report_failure(&err);

                let mut resp = Response::new(format!("Error: {err}\n").into());
                *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                resp
            }
        })
}

fn endpoint_metrics(data: Arc<Data>) -> Fallible<Response<Body>> {
    data.metrics.update_agent_status(
        &data.db,
        &data.agents.all()?.iter().collect::<Vec<&Agent>>(),
    )?;

    data.metrics.update_crates_lists(&data.db)?;

    let mut buffer = Vec::new();
    let families = prometheus::gather();
    TextEncoder::new().encode(&families, &mut buffer)?;
    Ok(Response::new(Body::from(buffer)))
}
