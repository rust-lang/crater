use errors::*;
use reqwest::{Client, ClientBuilder, RedirectPolicy, Response, StatusCode};

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

pub(crate) fn get(url: &str) -> Result<Response> {
    ::utils::try_hard(|| {
        let resp = HTTP_CLIENT.get(url).send()?;

        // Return an error if the response wasn't a 200 OK
        match resp.status() {
            StatusCode::Ok => Ok(resp),
            other => bail!("GET {} failed with status code {}", url, other),
        }
    })
}
