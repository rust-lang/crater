// FIXME Make these agree with cargo/rustup

use errors::*;
use std::env;
use std::path::PathBuf;

pub fn home_dir() -> Result<PathBuf> {
    if let Some(d) = env::home_dir() {
        return Ok(d);
    }
    bail!("no home directory");
}

pub fn cargo_home() -> Result<PathBuf> {
    if let Ok(p) = env::var("CARGO_HOME") {
        return Ok(PathBuf::from(p));
    }
    Ok(home_dir()?.join(".cargo"))
}

pub fn rustup_home() -> Result<PathBuf> {
    if let Ok(p) = env::var("CARGO_HOME") {
        return Ok(PathBuf::from(p));
    }
    Ok(home_dir()?.join(".cargo"))
}
