use config::Config;
use dirs::EXPERIMENT_DIR;
use errors::*;
use ex::{self, config_file, ExOpts, Experiment};
use file;
use serde_json;
use std::collections::HashMap;
use std::path::PathBuf;

fn server_data_file(name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(name).join("server_data.json")
}

#[derive(Serialize, Deserialize)]
pub struct ServerData {
    pub priority: i32,
}

pub struct ExperimentData {
    pub server_data: ServerData,
    pub experiment: Experiment,
}

impl ExperimentData {
    fn load(name: &str) -> Result<Self> {
        let path = server_data_file(name);
        Ok(ExperimentData {
            server_data: serde_json::from_str(&file::read_string(&path)?)?,
            experiment: Experiment::load(name)?,
        })
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
                info!("loading existing experiment {}...", name);
                let ex = ExperimentData::load(&name)?;
                data.insert(name, ex);
            }
        }

        Ok(Experiments { data })
    }

    pub fn exists(&self, name: &str) -> bool {
        self.data.contains_key(name)
    }

    pub fn create(&mut self, opts: ExOpts, config: &Config, priority: i32) -> Result<()> {
        let name = opts.name.clone();

        ex::define(opts, config)?;

        let data = ExperimentData {
            experiment: Experiment::load(&name)?,
            server_data: ServerData { priority },
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
}
