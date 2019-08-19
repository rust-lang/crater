use crate::prelude::*;
use http::{header::USER_AGENT, Method, StatusCode};
use reqwest::{Client, ClientBuilder, RedirectPolicy, RequestBuilder, Response};

const MAX_REDIRECTS: usize = 4;

#[derive(Debug, Fail)]
#[fail(display = "request to {} returned status code {}", url, status)]
pub struct InvalidStatusCode {
    url: String,
    status: StatusCode,
}

lazy_static! {
    static ref HTTP_SYNC_CLIENT: Client = setup_sync_client();
}

fn setup_sync_client() -> Client {
    ClientBuilder::new()
        .redirect(RedirectPolicy::limited(MAX_REDIRECTS))
        .build()
        .unwrap()
}

pub(crate) fn prepare_sync(method: Method, url: &str) -> RequestBuilder {
    HTTP_SYNC_CLIENT
        .request(method, url)
        .header(USER_AGENT, crate::USER_AGENT.clone())
}

pub(crate) fn get_sync(url: &str) -> Fallible<Response> {
    let resp = prepare_sync(Method::GET, url).send()?;

    // Return an error if the response wasn't a 200 OK
    match resp.status() {
        StatusCode::OK => Ok(resp),
        status => Err(InvalidStatusCode {
            url: url.to_string(),
            status,
        }
        .into()),
    }
}
