use prelude::*;
use reqwest::{Client, ClientBuilder, RedirectPolicy, Response, StatusCode};

const MAX_REDIRECTS: usize = 4;

#[derive(Debug, Fail)]
#[fail(
    display = "request to {} returned status code {}",
    url,
    status
)]
pub struct InvalidStatusCode {
    url: String,
    status: StatusCode,
}

lazy_static! {
    static ref HTTP_CLIENT: Client = setup_client();
}

fn setup_client() -> Client {
    ClientBuilder::new()
        .redirect(RedirectPolicy::limited(MAX_REDIRECTS))
        .build()
        .unwrap()
}

pub(crate) fn get(url: &str) -> Fallible<Response> {
    ::utils::try_hard(|| {
        let resp = HTTP_CLIENT.get(url).send()?;

        // Return an error if the response wasn't a 200 OK
        match resp.status() {
            StatusCode::OK => Ok(resp),
            status => Err(InvalidStatusCode {
                url: url.to_string(),
                status,
            }.into()),
        }
    })
}
