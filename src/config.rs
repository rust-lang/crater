use errors::*;
use ex::ExCrate;
use gh_mirrors;
use lists::Crate;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

static CONFIG_FILE: &'static str = "config.toml";

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CrateConfig {
    #[serde(default = "default_false")]
    skip: bool,
    #[serde(default = "default_false")]
    skip_tests: bool,
    #[serde(default = "default_false")]
    quiet: bool,
}

fn default_false() -> bool {
    false
}

pub enum CrateDetails<'a> {
    Version(&'a str),
    GitHubRepo(&'a str, &'a str),
}

pub trait GetDetails {
    fn get_details(&self) -> Option<CrateDetails>;
}

impl GetDetails for ExCrate {
    fn get_details(&self) -> Option<CrateDetails> {
        match *self {
            ExCrate::Version { ref name, .. } => Some(CrateDetails::Version(name)),
            ExCrate::Repo {
                ref org, ref name, ..
            } => Some(CrateDetails::GitHubRepo(org, name)),
        }
    }
}

impl GetDetails for Crate {
    fn get_details(&self) -> Option<CrateDetails> {
        match *self {
            Crate::Version { ref name, .. } => Some(CrateDetails::Version(name)),
            Crate::Repo { ref url } => {
                if let Ok((org, name)) = gh_mirrors::gh_url_to_org_and_name(url) {
                    Some(CrateDetails::GitHubRepo(org, name))
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    crates: HashMap<String, CrateConfig>,
    github_repos: HashMap<String, CrateConfig>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut buffer = String::new();
        File::open(CONFIG_FILE)?.read_to_string(&mut buffer)?;

        Ok(::toml::from_str(&buffer)?)
    }

    fn crate_config<C: GetDetails>(&self, c: &C) -> Option<&CrateConfig> {
        match c.get_details() {
            Some(CrateDetails::Version(name)) => self.crates.get(name),
            Some(CrateDetails::GitHubRepo(org, name)) => {
                let repo_name = format!("{}/{}", org, name);
                self.github_repos.get(&repo_name)
            }
            None => None,
        }
    }

    pub fn should_skip<C: GetDetails>(&self, c: &C) -> bool {
        self.crate_config(c).map(|c| c.skip).unwrap_or(false)
    }

    pub fn should_skip_tests<C: GetDetails>(&self, c: &C) -> bool {
        self.crate_config(c).map(|c| c.skip_tests).unwrap_or(false)
    }

    pub fn is_quiet<C: GetDetails>(&self, c: &C) -> bool {
        self.crate_config(c).map(|c| c.quiet).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use ex::ExCrate;

    #[test]
    fn test_config() {
        // A sample config file loaded from memory
        let config = concat!(
            "[crates]\n",
            "lazy_static = { skip = true }\n",
            "\n",
            "[github-repos]\n",
            "\"rust-lang/rust\" = { quiet = true }\n" // :(
        );

        let list: Config = ::toml::from_str(&config).unwrap();

        assert!(list.should_skip(&ExCrate::Version {
            name: "lazy_static".into(),
            version: "42".into(),
        }));
        assert!(!list.should_skip(&ExCrate::Version {
            name: "rand".into(),
            version: "42".into(),
        }));

        assert!(list.is_quiet(&ExCrate::Repo {
            org: "rust-lang".into(),
            name: "rust".into(),
            sha: None,
        }));
        assert!(!list.is_quiet(&ExCrate::Repo {
            org: "rust-lang".into(),
            name: "cargo".into(),
            sha: None,
        }));
    }
}
