use dl;
use errors::*;
use reqwest;
use std::collections::HashSet;
use std::io::Read;
use std::thread;
use std::time::Duration;

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
        for page in 1..(QUERIES_PER + 1) {
            let url = format!("{}&page={}", q, page);
            info!("downloading {}", url);

            let mut response = if page < 20 {
                    dl::download_limit(&url, 10000)
                } else {
                    dl::download_no_retry(&url)
                }
                .chain_err(|| "unable to query github for rust repos")?;

            // After some point, errors indicate the end of available results
            if page > 20 && *response.status() == reqwest::StatusCode::UnprocessableEntity {
                info!("error result. continuing");
                thread::sleep(Duration::from_secs(TIME_PER as u64));
                continue 'next_query;
            }

            let json: GitHubSearchPage = response.json()?;

            if json.items.is_empty() {
                info!("no results. continuing");
                thread::sleep(Duration::from_secs(TIME_PER as u64));
                continue 'next_query;
            }

            for item in json.items {
                info!("found rust repo {}", item.full_name);
                urls.insert(item.full_name);
            }

            thread::sleep(Duration::from_secs(TIME_PER as u64));
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

    let is_app = dl::download_no_retry(&url)
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
