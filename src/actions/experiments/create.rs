use actions::experiments::ExperimentError;
use chrono::Utc;
use config::Config;
use db::{Database, QueryUtils};
use experiments::{CapLints, CrateSelect, Experiment, GitHubIssue, Mode, Status};
use prelude::*;
use toolchain::Toolchain;

pub struct CreateExperiment {
    pub name: String,
    pub toolchains: [Toolchain; 2],
    pub mode: Mode,
    pub crates: CrateSelect,
    pub cap_lints: CapLints,
    pub priority: i32,
    pub github_issue: Option<GitHubIssue>,
}

impl CreateExperiment {
    #[cfg(test)]
    pub fn dummy(name: &str) -> Self {
        use toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

        CreateExperiment {
            name: name.to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
        }
    }

    pub fn apply(self, db: &Database, config: &Config) -> Fallible<()> {
        // Ensure no duplicate experiments are created
        if Experiment::exists(db, &self.name)? {
            return Err(ExperimentError::AlreadyExists(self.name).into());
        }

        // Ensure no experiment with duplicate toolchains is created
        if self.toolchains[0] == self.toolchains[1] {
            return Err(ExperimentError::DuplicateToolchains.into());
        }

        let crates = ::crates::lists::get_crates(self.crates, db, config)?;

        db.transaction(|transaction| {
            transaction.execute(
                "INSERT INTO experiments \
                 (name, mode, cap_lints, toolchain_start, toolchain_end, priority, created_at, \
                 status, github_issue, github_issue_url, github_issue_number) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11);",
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
                ],
            )?;

            for krate in &crates {
                let skipped = config.should_skip(krate) as i32;
                transaction.execute(
                    "INSERT INTO experiment_crates (experiment, crate, skipped) VALUES (?1, ?2, ?3);",
                    &[&self.name, &::serde_json::to_string(&krate)?, &skipped],
                )?;
            }

            Ok(())
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CreateExperiment;
    use actions::ExperimentError;
    use config::Config;
    use db::Database;
    use experiments::{CapLints, CrateSelect, Experiment, GitHubIssue, Mode, Status};
    use toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    #[test]
    fn test_creation() {
        let db = Database::temp().unwrap();
        let config = Config::default();

        ::crates::lists::setup_test_lists(&db, &config).unwrap();

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
        }.apply(&db, &config)
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
            ::crates::lists::get_crates(CrateSelect::Local, &db, &config).unwrap()
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
    }

    #[test]
    fn test_duplicate_toolchains() {
        let db = Database::temp().unwrap();
        let config = Config::default();

        ::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Ensure an experiment with duplicate toolchains can't be created
        let err = CreateExperiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), MAIN_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
        }.apply(&db, &config)
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

        ::crates::lists::setup_test_lists(&db, &config).unwrap();

        // The first experiment can be created successfully
        CreateExperiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            crates: CrateSelect::Local,
            cap_lints: CapLints::Forbid,
            priority: 0,
            github_issue: None,
        }.apply(&db, &config)
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
        }.apply(&db, &config)
        .unwrap_err();

        assert_eq!(
            err.downcast_ref(),
            Some(&ExperimentError::AlreadyExists("foo".into()))
        );
    }
}
