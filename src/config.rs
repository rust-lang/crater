use crate::crates::Crate;
use crate::prelude::*;
use crate::utils::size::Size;
use regex::Regex;
use serde_regex;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

fn default_config_file() -> PathBuf {
    env::var_os("CRATER_CONFIG")
        .unwrap_or_else(|| OsStr::new("config.toml").to_os_string())
        .into()
}

#[derive(Debug, Fail)]
#[fail(display = "the configuration file has errors")]
pub struct BadConfig;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CrateConfig {
    #[serde(default = "default_false")]
    pub skip: bool,
    #[serde(default = "default_false")]
    pub skip_tests: bool,
    #[serde(default = "default_false")]
    pub quiet: bool,
    #[serde(default = "default_false")]
    pub broken: bool,
}

fn default_false() -> bool {
    false
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ServerConfig {
    pub bot_acl: BotACL,
    pub labels: ServerLabels,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BotACL {
    pub rust_teams: bool,
    pub github: Vec<String>,
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
    pub local_crates: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SandboxConfig {
    pub memory_limit: Size,
    pub build_log_max_size: Size,
    pub build_log_max_lines: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ChunkConfig {
    pub chunk_size: i32,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub demo_crates: DemoCrates,
    pub crates: HashMap<String, CrateConfig>,
    pub github_repos: HashMap<String, CrateConfig>,
    pub local_crates: HashMap<String, CrateConfig>,
    pub server: ServerConfig,
    pub sandbox: SandboxConfig,
    pub chunk: ChunkConfig,
}

impl Config {
    pub fn load() -> Fallible<Self> {
        let buffer = Self::load_as_string(default_config_file())?;

        Ok(::toml::from_str(&buffer)?)
    }

    fn load_as_string(filename: PathBuf) -> Fallible<String> {
        let mut buffer = String::new();
        File::open(filename)?.read_to_string(&mut buffer)?;

        Ok(buffer)
    }

    fn crate_config(&self, c: &Crate) -> Option<&CrateConfig> {
        match *c {
            Crate::Registry(ref details) => self.crates.get(&details.name),
            Crate::GitHub(ref repo) => self.github_repos.get(&repo.slug()),
            Crate::Local(ref name) => self.local_crates.get(name),
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

    pub fn is_broken(&self, c: &Crate) -> bool {
        self.crate_config(c).map(|c| c.broken).unwrap_or(false)
    }

    pub fn demo_crates(&self) -> &DemoCrates {
        &self.demo_crates
    }

    pub fn chunk_size(&self) -> i32 {
        self.chunk.chunk_size
    }

    pub fn check(file: &Option<String>) -> Fallible<()> {
        if let Some(file) = file {
            Self::check_all(file.into())
        } else {
            Self::check_all(default_config_file())
        }
    }

    fn check_all(filename: PathBuf) -> Fallible<()> {
        use crate::experiments::CrateSelect;

        let buffer = Self::load_as_string(filename)?;
        let mut has_errors = Self::check_for_dup_keys(&buffer).is_err();
        let cfg: Self = ::toml::from_str(&buffer)?;
        let db = crate::db::Database::open()?;
        let crates = crate::crates::lists::get_crates(CrateSelect::Full, &db, &cfg)?;
        has_errors |= cfg.check_for_missing_crates(&crates).is_err();
        has_errors |= cfg.check_for_missing_repos(&crates).is_err();
        if has_errors {
            Err(BadConfig.into())
        } else {
            Ok(())
        }
    }

    fn check_for_dup_keys(buffer: &str) -> Fallible<()> {
        if let Err(e) = ::toml::from_str::<::toml::Value>(&buffer) {
            error!("got error parsing the config-file: {}", e);
            Err(e.into())
        } else {
            Ok(())
        }
    }

    fn check_for_missing_crates(&self, crates: &[Crate]) -> Fallible<()> {
        if self.crates.is_empty() {
            return Ok(());
        }

        let mut list_of_crates: HashSet<String> = HashSet::new();
        for krate in crates {
            let name = if let Crate::Registry(ref details) = krate {
                details.name.clone()
            } else {
                continue;
            };
            list_of_crates.insert(name);
        }

        let mut any_missing = false;
        for crate_name in self.crates.keys() {
            if !list_of_crates.contains(&*crate_name) {
                error!(
                    "check-config failed: crate `{}` is not available.",
                    crate_name
                );
                any_missing = true;
            }
        }
        if any_missing {
            Err(BadConfig.into())
        } else {
            Ok(())
        }
    }

    fn check_for_missing_repos(&self, crates: &[Crate]) -> Fallible<()> {
        if self.github_repos.is_empty() {
            return Ok(());
        }

        let mut list_of_crates: HashSet<String> = HashSet::new();
        for krate in crates {
            let name = if let Crate::GitHub(ref details) = krate {
                format!("{}/{}", details.org, details.name)
            } else {
                continue;
            };
            list_of_crates.insert(name);
        }

        let mut any_missing = false;
        for repo_name in self.github_repos.keys() {
            if !list_of_crates.contains(&*repo_name) {
                error!(
                    "check-config failed: GitHub repo `{}` is missing",
                    repo_name
                );
                any_missing = true;
            }
        }
        if any_missing {
            Err(BadConfig.into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
impl Default for Config {
    fn default() -> Self {
        Config {
            demo_crates: DemoCrates {
                crates: vec!["lazy_static".into()],
                github_repos: vec!["brson/hello-rs".into()],
                local_crates: vec![],
            },
            crates: HashMap::new(),
            github_repos: HashMap::new(),
            local_crates: HashMap::new(),
            sandbox: SandboxConfig {
                memory_limit: Size::Gigabytes(2),
                build_log_max_size: Size::Megabytes(1),
                build_log_max_lines: 1000,
            },
            server: ServerConfig {
                bot_acl: BotACL {
                    rust_teams: false,
                    github: vec![],
                },
                labels: ServerLabels {
                    remove: Regex::new("^$").unwrap(),
                    experiment_queued: "".into(),
                    experiment_completed: "".into(),
                },
            },
            chunk: ChunkConfig { chunk_size: 1 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use crate::crates::{Crate, GitHubRepo, RegistryCrate};

    #[test]
    fn test_config() {
        // A sample config file loaded from memory
        let config = concat!(
            "[server.bot-acl]\n",
            "rust-teams = false\n",
            "github = []\n",
            "[server.labels]\n",
            "remove = \"\"\n",
            "experiment-queued = \"\"\n",
            "experiment-completed = \"\"\n",
            "[demo-crates]\n",
            "crates = []\n",
            "github-repos = []\n",
            "local-crates = []\n",
            "[sandbox]\n",
            "memory-limit = \"2G\"\n",
            "build-log-max-size = \"2M\"\n",
            "build-log-max-lines = 1000\n",
            "[chunk]\n",
            "chunk-size = 32\n",
            "[crates]\n",
            "lazy_static = { skip = true }\n",
            "[github-repos]\n",
            "\"rust-lang/rust\" = { quiet = true }\n", // :(
            "[local-crates]\n"
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

        assert_eq!(list.chunk_size(), 32);
    }
}
