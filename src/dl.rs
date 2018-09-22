use errors::*;
use reqwest::{Client, ClientBuilder, RedirectPolicy, Response, StatusCode};
use utils;

const MAX_REDIRECTS: usize = 4;

lazy_static! {
    static ref HTTP_CLIENT: Client = setup_client();
}

fn setup_client() -> Client {
    ClientBuilder::new()
        .redirect(RedirectPolicy::limited(MAX_REDIRECTS))
        .build()
        .unwrap()
}

pub fn download(url: &str) -> Result<Response> {
    utils::try_hard(|| download_no_retry(url))
}

pub fn download_limit(url: &str, ms: usize) -> Result<Response> {
    utils::try_hard_limit(ms, || download_no_retry(url))
}

pub fn download_no_retry(url: &str) -> Result<Response> {
    let resp = HTTP_CLIENT.get(url).send()?;

    // Return an error if the response wasn't a 200 OK
    match resp.status() {
        StatusCode::Ok => Ok(resp),
        other => bail!("GET {} failed with status code {}", url, other),
    }
}
