use crate::actions::experiments::ExperimentError;
use crate::config::Config;
use crate::db::{Database, QueryUtils};
use crate::experiments::Experiment;
use crate::prelude::*;

pub struct DeleteExperiment {
    pub name: String,
}

impl DeleteExperiment {
    pub fn apply(self, db: &Database, _config: &Config) -> Fallible<()> {
        if !Experiment::exists(db, &self.name)? {
            return Err(ExperimentError::NotFound(self.name).into());
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
    use crate::actions::{CreateExperiment, ExperimentError};
    use crate::config::Config;
    use crate::db::Database;
    use crate::experiments::Experiment;

    #[test]
    fn test_delete_missing_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::default();

        let err = DeleteExperiment {
            name: "dummy".to_string(),
        }
        .apply(&db, &config)
        .unwrap_err();

        assert_eq!(
            err.downcast_ref(),
            Some(&ExperimentError::NotFound("dummy".into()))
        );
    }

    #[test]
    fn test_delete_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::default();

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment and make sure it exists
        CreateExperiment::dummy("dummy")
            .apply(&db, &config)
            .unwrap();
        assert!(Experiment::exists(&db, "dummy").unwrap());

        // Delete it and make sure it doesn't exist anymore
        DeleteExperiment {
            name: "dummy".to_string(),
        }
        .apply(&db, &config)
        .unwrap();
        assert!(!Experiment::exists(&db, "dummy").unwrap());
    }
}
