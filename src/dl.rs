use errors::*;
use std::cell::RefCell;
use url::Url;
use std::process::Command;
use util;

pub fn download(url: &str) -> Result<Vec<u8>> {
    util::try_hard(|| {
        download_no_retry(url)
    })
}

pub fn download_no_retry(url: &str) -> Result<Vec<u8>> {
    let out = Command::new("curl")
        .arg("-sSLf")
        .arg(url)
        .output()
        .chain_err(|| "unable to run curl")?;

    if !out.status.success() {
        return Err("failed to search github".into());
    }

    Ok(out.stdout)
}
