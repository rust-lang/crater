use crates::Crate;
use errors::*;
use regex::Regex;
use serde_regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

static CONFIG_FILE: &'static str = "config.toml";

#[derive(Clone, Serialize, Deserialize)]
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

#[derive(Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub labels: ServerLabels,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ServerLabels {
    #[serde(with = "serde_regex")]
    pub remove: Regex,
    pub experiment_queued: String,
    pub experiment_completed: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DemoCrates {
    pub crates: Vec<String>,
    pub github_repos: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    demo_crates: DemoCrates,
    crates: HashMap<String, CrateConfig>,
    github_repos: HashMap<String, CrateConfig>,
    pub server: ServerConfig,
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut buffer = String::new();
        File::open(CONFIG_FILE)?.read_to_string(&mut buffer)?;

        Ok(::toml::from_str(&buffer)?)
    }

    fn crate_config(&self, c: &Crate) -> Option<&CrateConfig> {
        match *c {
            Crate::Registry(ref details) => self.crates.get(&details.name),
            Crate::GitHub(ref repo) => self.github_repos.get(&repo.slug()),
        }
    }

    pub fn should_skip(&self, c: &Crate) -> bool {
        self.crate_config(c).map(|c| c.skip).unwrap_or(false)
    }

    pub fn should_skip_tests(&self, c: &Crate) -> bool {
        self.crate_config(c).map(|c| c.skip_tests).unwrap_or(false)
    }

    pub fn is_quiet(&self, c: &Crate) -> bool {
        self.crate_config(c).map(|c| c.quiet).unwrap_or(false)
    }

    pub fn demo_crates(&self) -> &DemoCrates {
        &self.demo_crates
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use crates::{Crate, GitHubRepo, RegistryCrate};

    #[test]
    fn test_config() {
        // A sample config file loaded from memory
        let config = concat!(
            "[server.labels]\n",
            "remove = \"\"\n",
            "experiment-queued = \"\"\n",
            "experiment-completed = \"\"\n",
            "[demo-crates]\n",
            "crates = []\n",
            "github-repos = []\n",
            "[crates]\n",
            "lazy_static = { skip = true }\n",
            "\n",
            "[github-repos]\n",
            "\"rust-lang/rust\" = { quiet = true }\n" // :(
        );

        let list: Config = ::toml::from_str(&config).unwrap();

        assert!(list.should_skip(&Crate::Registry(RegistryCrate {
            name: "lazy_static".into(),
            version: "42".into(),
        })));
        assert!(!list.should_skip(&Crate::Registry(RegistryCrate {
            name: "rand".into(),
            version: "42".into(),
        })));

        assert!(list.is_quiet(&Crate::GitHub(GitHubRepo {
            org: "rust-lang".into(),
            name: "rust".into(),
        })));
        assert!(!list.is_quiet(&Crate::GitHub(GitHubRepo {
            org: "rust-lang".into(),
            name: "cargo".into(),
        })));
    }
}
