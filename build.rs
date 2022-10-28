use std::process::Command;

fn cmd(args: &[&str]) -> Option<String> {
    if let Ok(out) = Command::new(args[0]).args(&args[1..]).output() {
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
    }

    None
}

fn get_git_sha() -> Option<String> {
    if let Some(sha) = cmd(&["git", "rev-parse", "--short", "HEAD"]) {
        let symbolic = cmd(&["git", "rev-parse", "--symbolic", "HEAD"]).unwrap();
        let symbolic_full = cmd(&["git", "rev-parse", "--symbolic-full-name", "HEAD"]).unwrap();

        println!("cargo:rerun-if-changed=.git/{symbolic}");
        if symbolic != symbolic_full {
            println!("cargo:rerun-if-changed=.git/{symbolic_full}");
        }

        Some(sha)
    } else {
        println!("cargo:warning=failed to get crater sha");
        None
    }
}

fn main() {
    let sha = format!("{:?}", get_git_sha());

    let output = std::env::var("OUT_DIR").unwrap();
    ::std::fs::write(format!("{output}/sha"), sha.as_bytes()).unwrap();
}
