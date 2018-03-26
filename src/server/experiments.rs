use chrono::{DateTime, Utc};
use config::Config;
use dirs::EXPERIMENT_DIR;
use errors::*;
use ex::{self, config_file, ExOpts, Experiment};
use file;
use serde_json;
use std::cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd};
use std::collections::HashMap;
use std::path::PathBuf;

fn server_data_file(name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(name).join("server_data.json")
}

#[derive(Serialize, Deserialize, Eq, PartialEq)]
pub enum Status {
    Queued,
    RunningOn(String),
    Completed,
}

#[derive(Serialize, Deserialize)]
pub struct ServerData {
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub github_issue: String,
    pub status: Status,
}

pub struct ExperimentData {
    pub server_data: ServerData,
    pub experiment: Experiment,
}

impl Ord for ExperimentData {
    fn cmp(&self, other: &ExperimentData) -> Ordering {
        self.server_data
            .priority
            .cmp(&other.server_data.priority)
            .then(
                self.server_data
                    .created_at
                    .cmp(&other.server_data.created_at)
                    .reverse(),
            )
    }
}

impl PartialOrd for ExperimentData {
    fn partial_cmp(&self, other: &ExperimentData) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for ExperimentData {}

impl PartialEq for ExperimentData {
    fn eq(&self, other: &ExperimentData) -> bool {
        self.experiment.name == other.experiment.name
    }
}

impl ExperimentData {
    fn load(name: &str) -> Result<Self> {
        let path = server_data_file(name);
        if path.is_file() {
            Ok(ExperimentData {
                server_data: serde_json::from_str(&file::read_string(&path)?)?,
                experiment: Experiment::load(name)?,
            })
        } else {
            bail!("not managed by the server");
        }
    }

    pub fn save(&self) -> Result<()> {
        let server_data_path = server_data_file(&self.experiment.name);
        let config_path = config_file(&self.experiment.name);

        file::write_string(
            &server_data_path,
            &serde_json::to_string(&self.server_data)?,
        )?;
        file::write_string(&config_path, &serde_json::to_string(&self.experiment)?)?;

        Ok(())
    }
}

pub struct Experiments {
    data: HashMap<String, ExperimentData>,
}

impl Experiments {
    pub fn new() -> Result<Self> {
        let mut data = HashMap::new();
        let base = EXPERIMENT_DIR.clone();

        for dir in ::std::fs::read_dir(&base)? {
            let name = dir?.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            if config_file(&name).exists() {
                match ExperimentData::load(&name) {
                    Ok(ex) => {
                        info!("loaded existing experiment {}", name);
                        data.insert(name, ex);
                    }
                    Err(err) => {
                        warn!("failed to load experiment {}: {}", name, err);
                    }
                }
            }
        }

        Ok(Experiments { data })
    }

    pub fn exists(&self, name: &str) -> bool {
        self.data.contains_key(name)
    }

    pub fn create(
        &mut self,
        opts: ExOpts,
        config: &Config,
        github_issue: &str,
        priority: i32,
    ) -> Result<()> {
        let name = opts.name.clone();

        ex::define(opts, config)?;

        let data = ExperimentData {
            experiment: Experiment::load(&name)?,
            server_data: ServerData {
                priority,
                created_at: Utc::now(),
                github_issue: github_issue.to_string(),
                status: Status::Queued,
            },
        };
        data.save()?;

        self.data.insert(name, data);
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<()> {
        ex::delete(name)?;

        self.data.remove(name);
        Ok(())
    }

    pub fn edit_data(&mut self, name: &str) -> Option<&mut ExperimentData> {
        self.data.get_mut(name)
    }

    pub fn run_by_agent(&self, agent_name: &str) -> Option<&str> {
        for (name, data) in &self.data {
            if let Status::RunningOn(ref running_on) = data.server_data.status {
                if running_on == agent_name {
                    return Some(name);
                }
            }
        }

        None
    }

    pub fn next(&mut self, agent_name: &str) -> Result<Option<(bool, &ExperimentData)>> {
        let mut candidate: Option<&mut ExperimentData> = None;

        for ex in self.data.values_mut() {
            // If an agent is already running an experiment don't assign a new one
            match ex.server_data.status {
                Status::Queued => {}
                Status::RunningOn(ref agent) if agent == agent_name => return Ok(Some((false, ex))),
                _ => continue,
            }

            if let Some(ref mut c) = candidate {
                if ex > c {
                    *c = ex;
                }
            } else {
                candidate = Some(ex)
            }
        }

        Ok(if let Some(c) = candidate {
            c.server_data.status = Status::RunningOn(agent_name.to_string());
            c.save()?;

            Some((true, c))
        } else {
            None
        })
    }
}
