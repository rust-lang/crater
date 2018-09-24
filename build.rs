extern crate git2;

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

    let target = std::env::var("TARGET").unwrap();

    let output = std::env::var("OUT_DIR").unwrap();
    ::std::fs::write(format!("{}/sha", output), sha.as_bytes()).unwrap();
    ::std::fs::write(format!("{}/target", output), target.as_bytes()).unwrap();

    // Avoid rebuilding everything when any file changes
    println!("cargo:rerun-if-changed=build.rs");
}
