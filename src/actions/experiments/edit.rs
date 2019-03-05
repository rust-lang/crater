use crate::actions::{experiments::ExperimentError, Action, ActionsCtx};
use crate::db::QueryUtils;
use crate::experiments::{Assignee, CapLints, CrateSelect, Experiment, Mode, Status};
use crate::prelude::*;
use crate::toolchain::Toolchain;

pub struct EditExperiment {
    pub name: String,
    pub toolchains: [Option<Toolchain>; 2],
    pub crates: Option<CrateSelect>,
    pub mode: Option<Mode>,
    pub cap_lints: Option<CapLints>,
    pub priority: Option<i32>,
    pub ignore_blacklist: Option<bool>,
    pub assign: Option<Assignee>,
}

impl EditExperiment {
    #[cfg(test)]
    pub fn dummy(name: &str) -> Self {
        EditExperiment {
            name: name.to_string(),
            toolchains: [None, None],
            mode: None,
            crates: None,
            cap_lints: None,
            priority: None,
            ignore_blacklist: None,
            assign: None,
        }
    }
}

impl Action for EditExperiment {
    fn apply(mut self, ctx: &ActionsCtx) -> Fallible<()> {
        let mut ex = match Experiment::get(&ctx.db, &self.name)? {
            Some(ex) => ex,
            None => return Err(ExperimentError::NotFound(self.name.clone()).into()),
        };

        // Ensure no change is made to running or complete experiments
        if ex.status != Status::Queued {
            return Err(ExperimentError::CanOnlyEditQueuedExperiments.into());
        }

        ctx.db.transaction(|t| {
            // Try to update both toolchains
            for (i, col) in ["toolchain_start", "toolchain_end"].iter().enumerate() {
                if let Some(tc) = self.toolchains[i].take() {
                    ex.toolchains[i] = tc;

                    // Ensure no duplicate toolchain is inserted
                    if ex.toolchains[0] == ex.toolchains[1] {
                        return Err(ExperimentError::DuplicateToolchains.into());
                    }

                    let changes = t.execute(
                        &format!("UPDATE experiments SET {} = ?1 WHERE name = ?2;", col),
                        &[&ex.toolchains[i].to_string(), &self.name],
                    )?;
                    assert_eq!(changes, 1);
                }
            }

            // Try to update the ignore_blacklist field
            // The list of skipped crates will be recalculated afterwards
            if let Some(ignore_blacklist) = self.ignore_blacklist {
                let changes = t.execute(
                    "UPDATE experiments SET ignore_blacklist = ?1 WHERE name = ?2;",
                    &[&ignore_blacklist, &self.name],
                )?;
                assert_eq!(changes, 1);
                ex.ignore_blacklist = ignore_blacklist;
            }

            // Try to update the list of crates
            // This is also done if ignore_blacklist is changed to recalculate the skipped crates
            let new_crates = if let Some(crates) = self.crates {
                Some(crate::crates::lists::get_crates(
                    crates,
                    &ctx.db,
                    &ctx.config,
                )?)
            } else if self.ignore_blacklist.is_some() {
                Some(ex.get_crates(&ctx.db)?)
            } else {
                None
            };
            if let Some(crates_vec) = new_crates {
                // Recreate the list of crates without checking if it was the same
                // This is done to allow reloading the list of crates in an existing experiment
                t.execute(
                    "DELETE FROM experiment_crates WHERE experiment = ?1;",
                    &[&self.name],
                )?;
                for krate in &crates_vec {
                    t.execute(
                        "INSERT INTO experiment_crates (experiment, crate, skipped) \
                         VALUES (?1, ?2, ?3);",
                        &[
                            &self.name,
                            &::serde_json::to_string(&krate)?,
                            &(!ex.ignore_blacklist && ctx.config.should_skip(krate)),
                        ],
                    )?;
                }
            }

            // Try to update the mode
            if let Some(mode) = self.mode {
                let changes = t.execute(
                    "UPDATE experiments SET mode = ?1 WHERE name = ?2;",
                    &[&mode.to_str(), &self.name],
                )?;
                assert_eq!(changes, 1);
                ex.mode = mode;
            }

            // Try to update the cap_lints
            if let Some(cap_lints) = self.cap_lints {
                let changes = t.execute(
                    "UPDATE experiments SET cap_lints = ?1 WHERE name = ?2;",
                    &[&cap_lints.to_str(), &self.name],
                )?;
                assert_eq!(changes, 1);
                ex.cap_lints = cap_lints;
            }

            // Try to update the priority
            if let Some(priority) = self.priority {
                let changes = t.execute(
                    "UPDATE experiments SET priority = ?1 WHERE name = ?2;",
                    &[&priority, &self.name],
                )?;
                assert_eq!(changes, 1);
                ex.priority = priority;
            }

            // Try to update the assignee
            if let Some(assign) = self.assign {
                let changes = t.execute(
                    "UPDATE experiments SET assigned_to = ?1 WHERE name = ?2;",
                    &[&assign.to_string(), &self.name],
                )?;
                assert_eq!(changes, 1);
                ex.assigned_to = Some(assign);
            }

            Ok(())
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::EditExperiment;
    use crate::actions::{Action, ActionsCtx, CreateExperiment, ExperimentError};
    use crate::config::{Config, CrateConfig};
    use crate::crates::Crate;
    use crate::db::{Database, QueryUtils};
    use crate::experiments::{Assignee, CapLints, CrateSelect, Experiment, Mode, Status};
    use crate::toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    #[test]
    fn test_edit_with_no_changes() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        CreateExperiment::dummy("foo").apply(&ctx).unwrap();
        EditExperiment::dummy("foo").apply(&ctx).unwrap();
    }

    #[test]
    fn test_edit_with_every_change() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create an experiment with the data we're going to change
        CreateExperiment {
            name: "foo".to_string(),
            toolchains: ["stable".parse().unwrap(), "beta".parse().unwrap()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::SmallRandom,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
            ignore_blacklist: false,
            assign: None,
        }
        .apply(&ctx)
        .unwrap();

        // Change everything!
        EditExperiment {
            name: "foo".to_string(),
            toolchains: [
                Some("nightly-1970-01-01".parse().unwrap()),
                Some("nightly-1970-01-02".parse().unwrap()),
            ],
            mode: Some(Mode::CheckOnly),
            crates: Some(CrateSelect::Local),
            cap_lints: Some(CapLints::Warn),
            priority: Some(10),
            ignore_blacklist: Some(true),
            assign: Some(Assignee::CLI),
        }
        .apply(&ctx)
        .unwrap();

        // And get the experiment to make sure data is changed
        let ex = Experiment::get(&db, "foo").unwrap().unwrap();

        assert_eq!(ex.toolchains[0], "nightly-1970-01-01".parse().unwrap());
        assert_eq!(ex.toolchains[1], "nightly-1970-01-02".parse().unwrap());
        assert_eq!(ex.mode, Mode::CheckOnly);
        assert_eq!(ex.cap_lints, CapLints::Warn);
        assert_eq!(ex.priority, 10);
        assert_eq!(ex.ignore_blacklist, true);
        assert_eq!(ex.assigned_to, Some(Assignee::CLI));

        assert_eq!(
            ex.get_crates(&ctx.db).unwrap(),
            crate::crates::lists::get_crates(CrateSelect::Local, &db, &config).unwrap()
        );
    }

    #[test]
    fn test_ignore_blacklist() {
        fn is_skipped(db: &Database, ex: &str, krate: &str) -> bool {
            let crates: Vec<Crate> = db
                .query(
                    "SELECT crate FROM experiment_crates WHERE experiment = ?1 AND skipped = 0",
                    &[&ex],
                    |row| {
                        let krate: String = row.get("crate");
                        serde_json::from_str(&krate).unwrap()
                    },
                )
                .unwrap();
            crates
                .iter()
                .find(|c| {
                    if let Crate::Local(name) = c {
                        name == krate
                    } else {
                        panic!("there should be no non-local crates")
                    }
                })
                .is_none()
        }

        let db = Database::temp().unwrap();
        let mut config = Config::default();
        config.local_crates.insert(
            "build-pass".into(),
            CrateConfig {
                skip: true,
                skip_tests: false,
                quiet: false,
                update_lockfile: false,
                broken: false,
            },
        );
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        CreateExperiment {
            ignore_blacklist: false,
            ..CreateExperiment::dummy("foo")
        }
        .apply(&ctx)
        .unwrap();
        assert!(is_skipped(&db, "foo", "build-pass"));

        EditExperiment {
            ignore_blacklist: Some(true),
            ..EditExperiment::dummy("foo")
        }
        .apply(&ctx)
        .unwrap();
        assert!(!is_skipped(&db, "foo", "build-pass"));

        EditExperiment {
            ignore_blacklist: Some(false),
            ..EditExperiment::dummy("foo")
        }
        .apply(&ctx)
        .unwrap();
        assert!(is_skipped(&db, "foo", "build-pass"));
    }

    #[test]
    fn test_duplicate_toolchains() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // First create an experiment
        let mut dummy = CreateExperiment::dummy("foo");
        dummy.toolchains = [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()];
        dummy.apply(&ctx).unwrap();

        // Then try to switch the second toolchain to MAIN_TOOLCHAIN
        let mut edit = EditExperiment::dummy("foo");
        edit.toolchains[1] = Some(MAIN_TOOLCHAIN.clone());

        let err = edit.apply(&ctx).unwrap_err();
        assert_eq!(
            err.downcast_ref(),
            Some(&ExperimentError::DuplicateToolchains)
        );
    }

    #[test]
    fn test_editing_missing_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        let err = EditExperiment::dummy("foo").apply(&ctx).unwrap_err();
        assert_eq!(
            err.downcast_ref(),
            Some(&ExperimentError::NotFound("foo".into()))
        );
    }

    #[test]
    fn test_editing_running_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create an experiment and set it to running
        CreateExperiment::dummy("foo").apply(&ctx).unwrap();
        let mut ex = Experiment::get(&db, "foo").unwrap().unwrap();
        ex.set_status(&db, Status::Running).unwrap();

        // Try to edit it
        let err = EditExperiment::dummy("foo").apply(&ctx).unwrap_err();
        assert_eq!(
            err.downcast_ref(),
            Some(&ExperimentError::CanOnlyEditQueuedExperiments)
        );
    }
}
