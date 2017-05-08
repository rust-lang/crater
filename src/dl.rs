use errors::*;
use std::cell::RefCell;
use std::process::Command;
use url::Url;
use util;

pub fn download(url: &str) -> Result<Vec<u8>> {
    util::try_hard(|| download_no_retry(url))
}

pub fn download_limit(url: &str, ms: usize) -> Result<Vec<u8>> {
    util::try_hard_limit(ms, || download_no_retry(url))
}

pub fn download_no_retry(url: &str) -> Result<Vec<u8>> {
    let out = Command::new("curl")
        .arg("-sSLf")
        .arg(url)
        .output()
        .chain_err(|| "unable to run curl")?;

    if !out.status.success() {
        bail!("failed to download {}", url);
    }

    Ok(out.stdout)
}
