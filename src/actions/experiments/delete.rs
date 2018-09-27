use config::Config;
use db::{Database, QueryUtils};
use errors::*;
use experiments::Experiment;

pub struct DeleteExperiment {
    pub name: String,
}

impl DeleteExperiment {
    pub fn apply(self, db: &Database, _config: &Config) -> Result<()> {
        if !Experiment::exists(db, &self.name)? {
            return Err(ErrorKind::ExperimentNotFound(self.name).into());
        }

        // This will also delete all the data related to this experiment, thanks to the foreign
        // keys in the SQLite database
        db.execute("DELETE FROM experiments WHERE name = ?1;", &[&self.name])?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DeleteExperiment;
    use actions::CreateExperiment;
    use config::Config;
    use db::Database;
    use errors::*;
    use experiments::Experiment;

    #[test]
    fn test_delete_missing_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::default();

        let err = DeleteExperiment {
            name: "dummy".to_string(),
        }.apply(&db, &config)
        .unwrap_err();

        match err.kind() {
            ErrorKind::ExperimentNotFound(name) => assert_eq!(name, "dummy"),
            other => panic!("unexpected error: {}", other),
        }
    }

    #[test]
    fn test_delete_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::default();

        ::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment and make sure it exists
        CreateExperiment::dummy("dummy")
            .apply(&db, &config)
            .unwrap();
        assert!(Experiment::exists(&db, "dummy").unwrap());

        // Delete it and make sure it doesn't exist anymore
        DeleteExperiment {
            name: "dummy".to_string(),
        }.apply(&db, &config)
        .unwrap();
        assert!(!Experiment::exists(&db, "dummy").unwrap());
    }
}
