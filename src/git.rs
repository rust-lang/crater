use errors::*;
use run::RunCommand;
use std::fs;
use std::path::Path;

pub fn shallow_clone_or_pull(url: &str, dir: &Path) -> Result<()> {
    let url = frob_url(url);

    if !dir.exists() {
        info!("cloning {} into {}", url, dir.display());
        let r = RunCommand::new("git")
            .args(&["clone", "--depth", "1", &url, &dir.to_string_lossy()])
            .run()
            .chain_err(|| format!("unable to clone {}", url));

        if r.is_err() && dir.exists() {
            fs::remove_dir_all(dir)?;
        }

        r
    } else {
        info!("pulling existing url {} into {}", url, dir.display());
        RunCommand::new("git")
            .args(&["fetch", "--all"])
            .cd(dir)
            .run()?;
        RunCommand::new("git")
            .args(&["reset", "--hard", "@{upstream}"])
            .cd(dir)
            .run()
            .chain_err(|| format!("unable to pull {}", url))
    }
}

fn frob_url(url: &str) -> String {
    // With https git will interactively ask for a password for private repos.
    // Switch to the unauthenticated git protocol to just generate an error instead.
    url.replace("https://", "git://")
}
