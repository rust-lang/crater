extern crate git2;

use std::{fs::File, io::Write};

fn main() {
    let mut sha = "None".to_string();
    if let Ok(repo) = git2::Repository::open(".") {
        if let Ok(rev) = repo.revparse_single("HEAD") {
            if let Ok(sha_buf) = rev.short_id() {
                if let Some(sha_str) = sha_buf.as_str() {
                    sha = format!("Some(\"{}\")", sha_str.to_string());
                }
            }
        }
    }

    File::create(format!("{}/sha", std::env::var("OUT_DIR").unwrap()))
        .unwrap()
        .write(sha.as_bytes())
        .unwrap();
}
