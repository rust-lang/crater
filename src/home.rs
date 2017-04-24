// FIXME Make these agree with cargo/rustup

use errors::*;
use std::env;
use std::path::PathBuf;

pub fn home_dir() -> Result<PathBuf> {
    env::home_dir()
        .ok_or_else(|| "no home directory".into())
}

pub fn cargo_home() -> Result<PathBuf> {
    env::var("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|_| Ok(home_dir()?.join(".cargo")))
}

pub fn rustup_home() -> Result<PathBuf> {
    env::var("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|_| Ok(home_dir()?.join(".rustup")))
}
