use dl;
use errors::*;
use hyper::header::{Link, RelationType};
use reqwest;
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
                          .map(|reset| UNIX_EPOCH + Duration::from_secs(reset.0))
                  } else {
                      None
                  })
}

fn gh_request(url: &str) -> Result<reqwest::Response> {
    // See https://github.com/Manishearth/rust-clippy/issues/1586
    #[cfg_attr(feature = "cargo-clippy", allow(never_loop))]
    loop {
        let response = dl::download_no_retry(url)?;
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

#[derive(Deserialize)]
struct GitHubSearchPage {
    items: Vec<GitHubSearchItem>,
}

#[derive(Deserialize)]
struct GitHubSearchItem {
    full_name: String,
}

const QUERIES_PER: usize = 40;
const TIME_PER: usize = 6;

pub fn get_candidate_repos() -> Result<Vec<String>> {
    info!("making up to {} queries. time: {} s. take a break",
          QUERIES.len() * QUERIES_PER,
          QUERIES.len() * QUERIES_PER * TIME_PER);

    let mut urls = HashSet::new();
    'next_query: for q in QUERIES {
        let mut url = q.to_string();
        'next_page: loop {
            info!("downloading {}", url);

            let mut response = util::try_hard_limit(10000, || gh_request(&url))
                .chain_err(|| "unable to query github for rust repos")?;

            if !response.status().is_success() {
                info!("error result: {}. continuing", response.status());
                continue 'next_query;
            }


            let json: GitHubSearchPage = response.json()?;

            if json.items.is_empty() {
                info!("no results. continuing");
                continue 'next_query;
            }

            for item in json.items {
                info!("found rust repo {}", item.full_name);
                urls.insert(item.full_name);
            }

            if let Some(links) = response.headers().get::<Link>() {
                for link in links.values() {
                    if link.rel() == Some(&[RelationType::Next]) {
                        url = link.link().to_string();
                        continue 'next_page;
                    }
                }
            }
            continue 'next_query;
        }
    }

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
