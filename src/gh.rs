use dl;
use std::thread;
use std::time::Duration;
use std::collections::HashSet;
use errors::*;

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

const QUERIES_PER: usize = 10;
const TIME_PER: usize = 6;

pub fn get_candidate_repos() -> Result<Vec<String>> {
    use json;
    use json::*;

    log!("making {} queries. time: {} s. take a break",
             QUERIES.len() * QUERIES_PER, QUERIES.len() * QUERIES_PER * TIME_PER);

    let mut urls = HashSet::new();
    for q in QUERIES {
        for page in 1 .. (QUERIES_PER + 1) {
            let url = format!("{}&page={}", q, page);
            log!("downloading {}", url);

            let buf = dl::download(&url)
                .chain_err(|| "unable to query github for rust repos")?;
            let response = String::from_utf8(buf)
                .chain_err(|| "non-utf8 github response")?;

            let json = json::parse(&response).chain_err(|| "parsing json")?;

            for item in json["items"].members() {
                if let Some(name) = item["full_name"].as_str() {
                    log!("found rust repo {}", name);
                    urls.insert(name.to_string());
                }            
            }

            thread::sleep(Duration::from_secs(TIME_PER as u64));
        }
    }

    let mut urls = urls.drain().collect::<Vec<_>>();
    urls.sort();

    Ok(urls)
}

pub fn is_rust_app(name: &str) -> Result<bool> {
    let url = format!("https://raw.githubusercontent.com/{}/master/Cargo.lock", name);
    log!("testing {}", url);
    let is_app = if let Ok(buf) = dl::download_no_retry(&url) {
        if let Ok(content) = String::from_utf8(buf) {
            // GitHub returns a successful result when the file doesn't exist
            if !content.contains("404: Not Found")
                && !content.is_empty() {
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if is_app {
        log!("{} contains a root lockfile at {}", name, url);
        Ok(true)
    } else {
        log!("{} does not contain a root lockfile", name);
        Ok(false)
    }
}
