use futures::future;
use hyper::header::ContentLength;
use hyper::server::{Request, Response};
use server::Data;
use server::auth::AuthDetails;
use server::http::{Context, ResponseFuture};
use std::sync::Arc;

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
pub fn config(
    _req: Request,
    _data: Arc<Data>,
    _ctx: Arc<Context>,
    auth: AuthDetails,
) -> ResponseFuture {
    let message = json!({
        "agent-name": auth.name,
    }).to_string();

    Box::new(future::ok(
        Response::new()
            .with_header(ContentLength(message.len() as u64))
            .with_body(message),
    ))
}
