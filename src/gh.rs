use dl;
use errors::*;
use hyper::header::{Link, RelationType};
use reqwest;
use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::io::Read;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use util;

// search repos for "language:rust stars:>0"
// curl -L "https://api.github.com/search/repositories?q=language:rust+stars:>0&sort=stars&page=1"
// rate limit is 10/minute
// see if it has a Cargo.lock in the root
// https://raw.githubusercontent.com/brson/cargobomb/master/Cargo.lock

const QUERIES: &'static [&'static str] = &[
    "https://api.github.com/search/repositories?q=language:rust&sort=stars&order=asc",
    "https://api.github.com/search/repositories?q=language:rust&sort=stars&order=desc",
    "https://api.github.com/search/repositories?q=language:rust&sort=updated&order=asc",
    "https://api.github.com/search/repositories?q=language:rust&sort=updated&order=desc",
    "https://api.github.com/search/repositories?q=language:rust+stars:0&sort=stars&order=asc",
    "https://api.github.com/search/repositories?q=language:rust+stars:0&sort=stars&order=desc",
    "https://api.github.com/search/repositories?q=language:rust+stars:1&sort=stars&order=asc",
    "https://api.github.com/search/repositories?q=language:rust+stars:1&sort=stars&order=desc",
    "https://api.github.com/search/repositories?q=language:rust+stars:2&sort=stars&order=asc",
    "https://api.github.com/search/repositories?q=language:rust+stars:2&sort=stars&order=desc",
    "https://api.github.com/search/repositories?q=language:rust+stars:3&sort=stars&order=asc",
    "https://api.github.com/search/repositories?q=language:rust+stars:3&sort=stars&order=desc",
    "https://api.github.com/search/repositories?q=language:rust+stars:>3&sort=stars&order=asc",
    "https://api.github.com/search/repositories?q=language:rust+stars:>3&sort=stars&order=desc",
];


use std::marker::PhantomData;
struct PageIter<T> {
    next_page: Option<String>,
    request_fn: fn(&str) -> Result<reqwest::Response>,
    _type: PhantomData<T>,
}

impl<T> PageIter<T> {
    fn new(url: &str, request_fn: fn(&str) -> Result<reqwest::Response>) -> Self {
        PageIter {
            next_page: Some(url.into()),
            _type: PhantomData,
            request_fn,
        }
    }
}

impl<T> Iterator for PageIter<T>
    where T: DeserializeOwned
{
    type Item = GitHubSearchPage<T>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(url) = self.next_page.take() {
            info!("downloading {}", url);
            let mut response = (self.request_fn)(&url).unwrap();
            let json: GitHubSearchPage<T> = response.json().unwrap();

            if let Some(links) = response.headers().get::<Link>() {
                for link in links.values() {
                    if link.rel() == Some(&[RelationType::Next]) {
                        self.next_page = Some(link.link().to_string());
                        break;
                    }
                }
            }
            Some(json)
        } else {
            None
        }
    }
}

fn gh_search<T>(url: &str) -> Box<Iterator<Item = T>>
    where T: DeserializeOwned + 'static
{
    Box::new(PageIter::new(url, gh_request).flat_map(|json| json.items))
}

header! { ( XRateLimitRemaining, "X-RateLimit-Remaining") => [u32] }
header! { ( XRateLimitReset, "X-RateLimit-Reset") => [u64] }

fn is_ratelimited(response: &reqwest::Response) -> Option<SystemTime> {
    if *response.status() != reqwest::StatusCode::Forbidden {
        return None;
    }
    let headers = response.headers();
    headers
        .get::<XRateLimitRemaining>()
        .and_then(|&XRateLimitRemaining(limit)| if limit == 0 {
                      headers
                          .get::<XRateLimitReset>()
                          // Add 1s to account for time divergence.
                          // If it isn't enough, we'll just retry anyway.
                          .map(|reset| UNIX_EPOCH + Duration::from_secs(reset.0+1))
                  } else {
                      None
                  })
}

/// Retry rate-limited requests, obeying `X-RateLimit-Remaining` and `X-RateLimit-Reset`.
fn retry_ratelimit(url: &str,
                   request_fn: &Fn(&str) -> Result<reqwest::Response>)
                   -> Result<reqwest::Response> {
    // See https://github.com/Manishearth/rust-clippy/issues/1586
    #[cfg_attr(feature = "cargo-clippy", allow(never_loop))]
    loop {
        let response = request_fn(url)?;
        if let Some(expiry) = is_ratelimited(&response) {
            if let Ok(duration) = expiry.duration_since(SystemTime::now()) {
                warn!("GitHub ratelimit, retrying in {}s", duration.as_secs());
                thread::sleep(duration);
                continue;
            }
        }
        return Ok(response);
    }
}

fn gh_request(url: &str) -> Result<reqwest::Response> {
    util::try_hard_limit(10000, || retry_ratelimit(url, &dl::download_no_retry))
        .and_then(|response| Ok(response.error_for_status()?))
        .chain_err(|| "unable to query github for rust repos")
}

#[derive(Deserialize)]
struct GitHubSearchPage<Item> {
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct GitHubRepositoryItem {
    full_name: String,
}

const QUERIES_PER: usize = 40;
const TIME_PER: usize = 6;

pub fn get_candidate_repos() -> Result<Vec<String>> {
    info!("making up to {} queries. time: {} s. take a break",
          QUERIES.len() * QUERIES_PER,
          QUERIES.len() * QUERIES_PER * TIME_PER);

    let mut urls: HashSet<_> = QUERIES
        .iter()
        .flat_map(|q| {
                      gh_search::<GitHubRepositoryItem>(q).map(|item| {
                                                                   info!("found rust repo {}",
                                                                         item.full_name);
                                                                   item.full_name
                                                               })
                  })
        .collect();

    let mut urls = urls.drain().collect::<Vec<_>>();
    urls.sort();

    Ok(urls)
}

pub fn is_rust_app(name: &str) -> Result<bool> {
    let url = format!("https://raw.githubusercontent.com/{}/master/Cargo.lock",
                      name);
    info!("testing {}", url);

    let is_app = gh_request(&url)
        .and_then(|mut response| {
                      let mut buf = String::new();
                      response.read_to_string(&mut buf)?;
                      // GitHub returns a successful result when the file doesn't exist
                      Ok(!buf.contains("404: Not Found") && !buf.is_empty())
                  })
        .unwrap_or(false);

    if is_app {
        info!("{} contains a root lockfile at {}", name, url);
        Ok(true)
    } else {
        info!("{} does not contain a root lockfile", name);
        Ok(false)
    }
}
