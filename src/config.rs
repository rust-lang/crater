use errors::*;
use ex::ExCrate;
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

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DemoCrates {
    pub crates: Vec<String>,
    pub github_repos: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    demo_crates: DemoCrates,
    crates: HashMap<String, CrateConfig>,
    github_repos: HashMap<String, CrateConfig>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut buffer = String::new();
        File::open(CONFIG_FILE)?.read_to_string(&mut buffer)?;

        Ok(::toml::from_str(&buffer)?)
    }

    fn crate_config(&self, ex: &ExCrate) -> Option<&CrateConfig> {
        match *ex {
            ExCrate::Version { ref name, .. } => self.crates.get(name),
            ExCrate::Repo {
                ref org, ref name, ..
            } => {
                let repo_name = format!("{}/{}", org, name);
                self.github_repos.get(&repo_name)
            }
        }
    }

    pub fn should_skip(&self, ex: &ExCrate) -> bool {
        self.crate_config(ex).map(|c| c.skip).unwrap_or(false)
    }

    pub fn should_skip_tests(&self, ex: &ExCrate) -> bool {
        self.crate_config(ex).map(|c| c.skip_tests).unwrap_or(false)
    }

    pub fn is_quiet(&self, ex: &ExCrate) -> bool {
        self.crate_config(ex).map(|c| c.quiet).unwrap_or(false)
    }

    pub fn demo_crates(&self) -> &DemoCrates {
        &self.demo_crates
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
            sha: "0".into(),
        }));
        assert!(!list.is_quiet(&ExCrate::Repo {
            org: "rust-lang".into(),
            name: "cargo".into(),
            sha: "0".into(),
        }));
    }
}
