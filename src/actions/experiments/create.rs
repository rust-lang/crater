use crate::actions::{experiments::ExperimentError, Action, ActionsCtx};
use crate::crates::Crate;
use crate::db::QueryUtils;
use crate::experiments::{CapLints, CrateSelect, Experiment, GitHubIssue, Mode, Status};
use crate::prelude::*;
use crate::toolchain::Toolchain;
use chrono::Utc;

static CHUNK_SIZE: usize = 2;

pub struct CreateExperiment {
    pub name: String,
    pub toolchains: [Toolchain; 2],
    pub mode: Mode,
    pub crates: CrateSelect,
    pub cap_lints: CapLints,
    pub priority: i32,
    pub github_issue: Option<GitHubIssue>,
    pub ignore_blacklist: bool,
}

impl CreateExperiment {
    #[cfg(test)]
    pub fn dummy(name: &str) -> Self {
        use crate::toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

        CreateExperiment {
            name: name.to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
            ignore_blacklist: false,
        }
    }
}

fn get_portion<T: std::clone::Clone>(vec: &Vec<T>, i: usize) -> Vec<T> {
    if i == vec.len() / CHUNK_SIZE {
        vec[i * CHUNK_SIZE..].to_vec()
    } else {
        vec[i * CHUNK_SIZE..i * CHUNK_SIZE + CHUNK_SIZE].to_vec()
    }
}

impl CreateExperiment {
    fn create_children(&self, ctx: &ActionsCtx, name: String, crates: Vec<Crate>) -> Fallible<()> {
        ctx.db.transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO experiment_chunks \
                    (name, mode, cap_lints, toolchain_start, toolchain_end, priority, created_at, \
                    status, github_issue, github_issue_url, github_issue_number, ignore_blacklist, parent) \
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13);",
                    &[
                        &name,
                        &self.mode.to_str(),
                        &self.cap_lints.to_str(),
                        &self.toolchains[0].to_string(),
                        &self.toolchains[1].to_string(),
                        &self.priority,
                        &Utc::now(),
                        &Status::Queued.to_str(),
                        &self.github_issue.as_ref().map(|i| i.api_url.as_str()),
                        &self.github_issue.as_ref().map(|i| i.html_url.as_str()),
                        &self.github_issue.as_ref().map(|i| i.number),
                        &self.ignore_blacklist,
                        &self.name,
                    ],
                )?;

                for krate in &crates {
                    let skipped = !self.ignore_blacklist && ctx.config.should_skip(krate);
                    transaction.execute(
                        "INSERT INTO experiment_chunk_crates (experiment_chunk, crate, skipped) VALUES (?1, ?2, ?3);",
                        &[&name, &::serde_json::to_string(&krate)?, &skipped],
                    )?;
                }

                Ok(())
            })?;
        Ok(())
    }
}

impl Action for CreateExperiment {
    fn apply(self, ctx: &ActionsCtx) -> Fallible<()> {
        // Ensure no duplicate experiments are created
        if Experiment::exists(&ctx.db, &self.name)? {
            return Err(ExperimentError::AlreadyExists(self.name.clone()).into());
        }

        // Ensure no experiment with duplicate toolchains is created
        if self.toolchains[0] == self.toolchains[1] {
            return Err(ExperimentError::DuplicateToolchains.into());
        }

        let crates = crate::crates::lists::get_crates(self.crates, &ctx.db, &ctx.config)?;
        ctx.db.transaction(|transaction| {
                  transaction.execute(
                      "INSERT INTO experiments \
                      (name, mode, cap_lints, toolchain_start, toolchain_end, priority, created_at, \
                      status, github_issue, github_issue_url, github_issue_number, children, ignore_blacklist) \
                      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13);",
                      &[
                          &self.name,
                          &self.mode.to_str(),
                          &self.cap_lints.to_str(),
                          &self.toolchains[0].to_string(),
                          &self.toolchains[1].to_string(),
                          &self.priority,
                          &Utc::now(),
                          &Status::Queued.to_str(),
                          &self.github_issue.as_ref().map(|i| i.api_url.as_str()),
                          &self.github_issue.as_ref().map(|i| i.html_url.as_str()),
                          &self.github_issue.as_ref().map(|i| i.number),
                          &((crates.len() / CHUNK_SIZE + 1) as i32),
                          &self.ignore_blacklist,
                      ],
                  )?;

                  for krate in &crates {
                      let skipped = !self.ignore_blacklist && ctx.config.should_skip(krate);
                      transaction.execute(
                          "INSERT INTO experiment_crates (experiment, crate, skipped) VALUES (?1, ?2, ?3);",
                          &[&self.name, &::serde_json::to_string(&krate)?, &skipped],
                      )?;
                  }

                  Ok(())
              })?;

        for i in 0..crates.len() / CHUNK_SIZE + 1 {
            let name = i.to_string()
                + "/"
                + &(crates.len() / CHUNK_SIZE).to_string()
                + "-chunk-"
                + &self.name;
            let crat = get_portion(&crates, i);
            self.create_children(ctx, name, crat)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CreateExperiment;
    use crate::actions::{Action, ActionsCtx, ExperimentError};
    use crate::config::{Config, CrateConfig};
    use crate::crates::Crate;
    use crate::db::{Database, QueryUtils};
    use crate::experiments::{CapLints, CrateSelect, Experiment, GitHubIssue, Mode, Status};
    use crate::toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    #[test]
    fn test_creation() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        let api_url = "https://api.github.com/repos/example/example/issues/10";
        let html_url = "https://github.com/example/example/issue/10";

        CreateExperiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 5,
            github_issue: Some(GitHubIssue {
                api_url: api_url.to_string(),
                html_url: html_url.to_string(),
                number: 10,
            }),
            ignore_blacklist: true,
        }
        .apply(&ctx)
        .unwrap();

        let ex = Experiment::get(&db, "foo").unwrap().unwrap();
        assert_eq!(ex.name.as_str(), "foo");
        assert_eq!(
            ex.toolchains,
            [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()]
        );
        assert_eq!(ex.mode, Mode::BuildAndTest);
        assert_eq!(
            ex.crates,
            crate::crates::lists::get_crates(CrateSelect::Local, &db, &config).unwrap()
        );
        assert_eq!(ex.cap_lints, CapLints::Forbid);
        assert_eq!(ex.github_issue.as_ref().unwrap().api_url.as_str(), api_url);
        assert_eq!(
            ex.github_issue.as_ref().unwrap().html_url.as_str(),
            html_url
        );
        assert_eq!(ex.github_issue.as_ref().unwrap().number, 10);
        assert_eq!(ex.priority, 5);
        assert_eq!(ex.status, Status::Queued);
        assert!(ex.assigned_to.is_none());
        assert!(ex.ignore_blacklist);
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

        CreateExperiment {
            ignore_blacklist: true,
            ..CreateExperiment::dummy("bar")
        }
        .apply(&ctx)
        .unwrap();
        assert!(!is_skipped(&db, "bar", "build-pass"));
    }

    #[test]
    fn test_duplicate_toolchains() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Ensure an experiment with duplicate toolchains can't be created
        let err = CreateExperiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), MAIN_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
            ignore_blacklist: false,
        }
        .apply(&ctx)
        .unwrap_err();

        assert_eq!(
            err.downcast_ref(),
            Some(&ExperimentError::DuplicateToolchains)
        );
    }

    #[test]
    fn test_duplicate_name() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // The first experiment can be created successfully
        CreateExperiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
            ignore_blacklist: false,
        }
        .apply(&ctx)
        .unwrap();

        // While the second one fails
        let err = CreateExperiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
            ignore_blacklist: false,
        }
        .apply(&ctx)
        .unwrap_err();

        assert_eq!(
            err.downcast_ref(),
            Some(&ExperimentError::AlreadyExists("foo".into()))
        );
    }
}
