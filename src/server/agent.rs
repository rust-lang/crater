use hyper::server::{Request, Response};
use server::Data;
use server::auth::AuthDetails;
use server::http::{Context, ResponseExt, ResponseFuture};
use std::sync::Arc;

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
pub fn config(
    _req: Request,
    _data: Arc<Data>,
    _ctx: Arc<Context>,
    auth: AuthDetails,
) -> ResponseFuture {
    Response::json(&json!({
        "agent-name": auth.name,
    })).unwrap()
        .as_future()
}
