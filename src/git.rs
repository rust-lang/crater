use errors::*;
use run::RunCommand;
use std::fs;
use std::path::Path;
use util;

pub fn shallow_clone_or_pull(url: &str, dir: &Path) -> Result<()> {
    let url = frob_url(url);

    if !dir.exists() {
        info!("cloning {} into {}", url, dir.display());
        let r = RunCommand::new(
            "git",
            &["clone", "--depth", "1", &url, &dir.to_string_lossy()],
        ).run()
            .chain_err(|| format!("unable to clone {}", url));

        if r.is_err() && dir.exists() {
            fs::remove_dir_all(dir)?;
        }

        r
    } else {
        info!("pulling existing url {} into {}", url, dir.display());
        RunCommand::new("git", &["pull"])
            .cd(dir)
            .run()
            .chain_err(|| format!("unable to pull {}", url))
    }
}

/// Ensure that a commit exists locally in a shallow-cloned repo. This will
/// first check whether it does, and if not do increasingly deep clones until it
/// finds the commit.
pub fn shallow_fetch_sha(url: &str, dir: &Path, sha: &str) -> Result<()> {
    let url = frob_url(url);

    info!("ensuring sha {} in {}", sha, url);
    let depths = &[1, 10, 100, 1000];

    let exists = || {
        if dir.exists() {
            RunCommand::new("git", &["log", sha]).cd(dir).run().is_ok()
        } else {
            false
        }
    };

    if exists() {
        return Ok(());
    }

    for depth in depths {
        util::try_hard(|| {
            RunCommand::new(
                "git",
                &[
                    "clone",
                    "--depth",
                    &format!("{}", depth),
                    &url,
                    &dir.to_string_lossy(),
                ],
            ).run()
        }).chain_err(|| format!("unable to clone {}", url))?;

        if exists() {
            return Ok(());
        }
    }

    util::try_hard(|| RunCommand::new("git", &["clone", &url, &dir.to_string_lossy()]).run())
        .chain_err(|| format!("unable to clone {}", url))?;

    if !exists() {
        Err(format!("commit {} does not exist in {}", sha, url).into())
    } else {
        Ok(())
    }
}

pub fn reset_to_sha(dir: &Path, sha: &str) -> Result<()> {
    RunCommand::new("git", &["reset", "--hard", sha])
        .cd(dir)
        .run()
        .chain_err(|| format!("unable to reset {} to {}", dir.display(), sha))
}

fn frob_url(url: &str) -> String {
    // With https git will interactively ask for a password for private repos.
    // Switch to the unauthenticated git protocol to just generate an error instead.
    url.replace("https://", "git://")
}
