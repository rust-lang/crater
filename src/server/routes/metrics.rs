use crate::prelude::*;
use http::{Response, StatusCode};
use hyper::Body;
use prometheus::{Encoder, TextEncoder};
use warp::{self, Filter, Rejection};

pub fn routes() -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    warp::get2()
        .and(warp::path::end())
        .map(|| match endpoint_metrics() {
            Ok(resp) => resp,
            Err(err) => {
                error!("error while processing metrics");
                crate::utils::report_failure(&err);

                let mut resp = Response::new(format!("Error: {}\n", err).into());
                *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                resp
            }
        })
}

fn endpoint_metrics() -> Fallible<Response<Body>> {
    let mut buffer = Vec::new();
    let families = prometheus::gather();
    TextEncoder::new().encode(&families, &mut buffer)?;
    Ok(Response::new(Body::from(buffer)))
}
