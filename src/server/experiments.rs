use chrono::{DateTime, Utc};
use config::Config;
use crates::Crate;
use errors::*;
use ex::{self, ExCapLints, ExCrateSelect, ExMode, Experiment};
use rusqlite::Row;
use serde_json;
use server::db::{Database, QueryUtils};
use toolchain::Toolchain;

string_enum!(pub enum Status {
    Queued => "queued",
    Running => "running",
    NeedsReport => "needs-report",
    GeneratingReport => "generating-report",
    ReportFailed => "report-failed",
    Completed => "completed",
});

pub struct GitHubIssue {
    pub api_url: String,
    pub html_url: String,
    pub number: i32,
}

pub struct ServerData {
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub github_issue: Option<GitHubIssue>,
    pub status: Status,
    pub assigned_to: Option<String>,
    pub report_url: Option<String>,
}

pub struct ExperimentData {
    pub server_data: ServerData,
    pub experiment: Experiment,
}

impl ExperimentData {
    pub fn set_status(&mut self, db: &Database, status: Status) -> Result<()> {
        db.execute(
            "UPDATE experiments SET status = ?1 WHERE name = ?2;",
            &[&status.to_str(), &self.experiment.name.as_str()],
        )?;

        let now = Utc::now();

        // Check if the new status is "running" and there is no starting date
        if status == Status::Running && self.server_data.started_at.is_none() {
            db.execute(
                "UPDATE experiments SET started_at = ?1 WHERE name = ?2;",
                &[&now, &self.experiment.name.as_str()],
            )?;
            self.server_data.started_at = Some(now);
        // Check if the old status was "running" and there is no completed date
        } else if self.server_data.status == Status::Running
            && self.server_data.completed_at.is_none()
        {
            db.execute(
                "UPDATE experiments SET completed_at = ?1 WHERE name = ?2;",
                &[&now, &self.experiment.name.as_str()],
            )?;
            self.server_data.completed_at = Some(now);
        }

        self.server_data.status = status;
        Ok(())
    }

    pub fn set_assigned_to(&mut self, db: &Database, assigned_to: Option<String>) -> Result<()> {
        db.execute(
            "UPDATE experiments SET assigned_to = ?1 WHERE name = ?2;",
            &[&assigned_to, &self.experiment.name.as_str()],
        )?;
        self.server_data.assigned_to = assigned_to;
        Ok(())
    }

    pub fn set_mode(&mut self, db: &Database, mode: ExMode) -> Result<()> {
        db.execute(
            "UPDATE experiments SET mode = ?1 WHERE name = ?2;",
            &[&mode.to_str(), &self.experiment.name.as_str()],
        )?;
        self.experiment.mode = mode;
        Ok(())
    }

    pub fn set_cap_lints(&mut self, db: &Database, cap_lints: ExCapLints) -> Result<()> {
        db.execute(
            "UPDATE experiments SET cap_lints = ?1 WHERE name = ?2;",
            &[&cap_lints.to_str(), &self.experiment.name.as_str()],
        )?;
        self.experiment.cap_lints = cap_lints;
        Ok(())
    }

    pub fn set_priority(&mut self, db: &Database, priority: i32) -> Result<()> {
        db.execute(
            "UPDATE experiments SET priority = ?1 WHERE name = ?2;",
            &[&priority, &self.experiment.name.as_str()],
        )?;
        self.server_data.priority = priority;
        Ok(())
    }

    pub fn set_crates(&mut self, db: &Database, config: &Config, crates: Vec<Crate>) -> Result<()> {
        db.transaction(|transaction| {
            transaction.execute(
                "DELETE FROM experiment_crates WHERE experiment = ?1;",
                &[&self.experiment.name.as_str()],
            )?;

            for krate in &crates {
                transaction.execute(
                    "INSERT INTO experiment_crates (experiment, crate, skipped) \
                     VALUES (?1, ?2, ?3);",
                    &[
                        &self.experiment.name.as_str(),
                        &serde_json::to_string(&krate)?,
                        &config.should_skip(krate),
                    ],
                )?;
            }

            Ok(())
        })?;
        self.experiment.crates = crates;
        Ok(())
    }

    pub fn set_start_toolchain(&mut self, db: &Database, start: Toolchain) -> Result<()> {
        self.experiment.toolchains[0] = start;
        self.experiment.validate()?;

        db.execute(
            "UPDATE experiments SET toolchain_start = ?1 WHERE name = ?2;",
            &[
                &self.experiment.toolchains[0].to_string(),
                &self.experiment.name.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn set_end_toolchain(&mut self, db: &Database, end: Toolchain) -> Result<()> {
        self.experiment.toolchains[1] = end;
        self.experiment.validate()?;

        db.execute(
            "UPDATE experiments SET toolchain_end = ?1 WHERE name = ?2;",
            &[
                &self.experiment.toolchains[1].to_string(),
                &self.experiment.name.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn set_report_url(&mut self, db: &Database, url: &str) -> Result<()> {
        db.execute(
            "UPDATE experiments SET report_url = ?1 WHERE name = ?2;",
            &[&url, &self.experiment.name.as_str()],
        )?;
        self.server_data.report_url = Some(url.to_string());
        Ok(())
    }

    pub fn raw_progress(&self, db: &Database) -> Result<(u32, u32)> {
        let results_len: u32 = db
            .get_row(
                "SELECT COUNT(*) AS count FROM results WHERE experiment = ?1;",
                &[&self.experiment.name.as_str()],
                |r| r.get("count"),
            )?.unwrap();

        let crates_len: u32 = db
            .get_row(
                "SELECT COUNT(*) AS count FROM experiment_crates \
                 WHERE experiment = ?1 AND skipped = 0;",
                &[&self.experiment.name.as_str()],
                |r| r.get("count"),
            )?.unwrap();

        Ok((results_len, crates_len * 2))
    }

    pub fn progress(&self, db: &Database) -> Result<u8> {
        let (results_len, crates_len) = self.raw_progress(db)?;

        if crates_len != 0 {
            Ok((results_len as f32 * 100.0 / crates_len as f32).ceil() as u8)
        } else {
            Ok(0)
        }
    }

    pub fn remove_completed_crates(&mut self, db: &Database) -> Result<()> {
        // FIXME: optimize this
        let mut new_crates = Vec::with_capacity(self.experiment.crates.len());
        for krate in self.experiment.crates.drain(..) {
            let results_len: u32 = db
                .get_row(
                    "SELECT COUNT(*) AS count FROM results \
                     WHERE experiment = ?1 AND crate = ?2;",
                    &[
                        &self.experiment.name.as_str(),
                        &serde_json::to_string(&krate)?,
                    ],
                    |r| r.get("count"),
                )?.unwrap();

            if results_len < 2 {
                new_crates.push(krate);
            }
        }

        self.experiment.crates = new_crates;
        Ok(())
    }
}

struct ExperimentDBRecord {
    name: String,
    mode: String,
    cap_lints: String,
    toolchain_start: String,
    toolchain_end: String,
    priority: i32,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    github_issue: Option<String>,
    github_issue_url: Option<String>,
    github_issue_number: Option<i32>,
    status: String,
    assigned_to: Option<String>,
    report_url: Option<String>,
}

impl ExperimentDBRecord {
    fn from_row(row: &Row) -> Self {
        ExperimentDBRecord {
            name: row.get("name"),
            mode: row.get("mode"),
            cap_lints: row.get("cap_lints"),
            toolchain_start: row.get("toolchain_start"),
            toolchain_end: row.get("toolchain_end"),
            priority: row.get("priority"),
            created_at: row.get("created_at"),
            started_at: row.get("started_at"),
            completed_at: row.get("completed_at"),
            status: row.get("status"),
            github_issue: row.get("github_issue"),
            github_issue_url: row.get("github_issue_url"),
            github_issue_number: row.get("github_issue_number"),
            assigned_to: row.get("assigned_to"),
            report_url: row.get("report_url"),
        }
    }

    fn into_experiment_data(self, db: &Database) -> Result<ExperimentData> {
        let crates = db
            .query(
                "SELECT crate FROM experiment_crates WHERE experiment = ?1",
                &[&self.name],
                |r| {
                    let value: String = r.get("crate");
                    Ok(serde_json::from_str(&value)?)
                },
            )?.into_iter()
            .collect::<Result<Vec<Crate>>>()?;

        Ok(ExperimentData {
            experiment: Experiment {
                name: self.name,
                crates,
                toolchains: [self.toolchain_start.parse()?, self.toolchain_end.parse()?],
                cap_lints: self.cap_lints.parse()?,
                mode: self.mode.parse()?,
            },
            server_data: ServerData {
                priority: self.priority,
                created_at: self.created_at,
                started_at: self.started_at,
                completed_at: self.completed_at,
                github_issue: if let (Some(api_url), Some(html_url), Some(number)) = (
                    self.github_issue,
                    self.github_issue_url,
                    self.github_issue_number,
                ) {
                    Some(GitHubIssue {
                        api_url,
                        html_url,
                        number,
                    })
                } else {
                    None
                },
                assigned_to: self.assigned_to,
                status: self.status.parse()?,
                report_url: self.report_url,
            },
        })
    }
}

#[derive(Clone)]
pub struct Experiments {
    db: Database,
}

impl Experiments {
    pub fn new(db: Database) -> Self {
        Experiments { db }
    }

    pub fn exists(&self, name: &str) -> Result<bool> {
        self.db
            .exists("SELECT rowid FROM experiments WHERE name = ?1;", &[&name])
    }

    #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
    pub fn create(
        &self,
        name: &str,
        toolchain_start: &Toolchain,
        toolchain_end: &Toolchain,
        mode: ExMode,
        crates: ExCrateSelect,
        cap_lints: ExCapLints,
        config: &Config,
        github_issue: Option<&str>,
        github_issue_url: Option<&str>,
        github_issue_number: Option<i32>,
        priority: i32,
    ) -> Result<()> {
        self.db.transaction(|transaction| {
            let crates = ex::get_crates(crates, config)?;

            // First of all, validate if the experiment is valid
            Experiment {
                name: name.to_string(),
                crates: crates.clone(),
                toolchains: [toolchain_start.clone(), toolchain_end.clone()],
                mode,
                cap_lints,
            }.validate()?;

            transaction.execute(
                "INSERT INTO experiments \
                 (name, mode, cap_lints, toolchain_start, toolchain_end, priority, created_at, \
                 status, github_issue, github_issue_url, github_issue_number) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11);",
                &[
                    &name,
                    &mode.to_str(),
                    &cap_lints.to_str(),
                    &toolchain_start.to_string(),
                    &toolchain_end.to_string(),
                    &priority,
                    &Utc::now(),
                    &"queued",
                    &github_issue,
                    &github_issue_url,
                    &github_issue_number,
                ],
            )?;

            for krate in &crates {
                let skipped = config.should_skip(krate) as i32;
                transaction.execute(
                    "INSERT INTO experiment_crates (experiment, crate, skipped) VALUES (?1, ?2, ?3);",
                    &[&name, &serde_json::to_string(&krate)?, &skipped],
                )?;
            }

            Ok(())
        })
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        // This will also delete all the data related to this experiment
        self.db
            .execute("DELETE FROM experiments WHERE name = ?1;", &[&name])
    }

    pub fn get(&self, name: &str) -> Result<Option<ExperimentData>> {
        let record = self.db.get_row(
            "SELECT * FROM experiments WHERE name = ?1;",
            &[&name],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            Ok(Some(record.into_experiment_data(&self.db)?))
        } else {
            Ok(None)
        }
    }

    pub fn all(&self) -> Result<Vec<ExperimentData>> {
        let records = self.db.query(
            "SELECT * FROM experiments ORDER BY priority DESC, created_at;",
            &[],
            |r| ExperimentDBRecord::from_row(r),
        )?;
        records
            .into_iter()
            .map(|record| record.into_experiment_data(&self.db))
            .collect::<Result<_>>()
    }

    pub fn run_by_agent(&self, agent: &str) -> Result<Option<ExperimentData>> {
        let record = self.db.get_row(
            "SELECT * FROM experiments \
             WHERE status = \"running\" AND assigned_to = ?1;",
            &[&agent],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            Ok(Some(record.into_experiment_data(&self.db)?))
        } else {
            Ok(None)
        }
    }

    pub fn first_by_status(&self, status: Status) -> Result<Option<ExperimentData>> {
        let record = self.db.get_row(
            "SELECT * FROM experiments \
             WHERE status = ?1 \
             ORDER BY priority DESC, created_at;",
            &[&status.to_str()],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            Ok(Some(record.into_experiment_data(&self.db)?))
        } else {
            Ok(None)
        }
    }

    pub fn next(&self, agent: &str) -> Result<Option<(bool, ExperimentData)>> {
        // Avoid assigning two experiments to the same agent
        if let Some(experiment) = self.run_by_agent(agent)? {
            return Ok(Some((false, experiment)));
        }

        let record = self.db.get_row(
            "SELECT * FROM experiments \
             WHERE status = \"queued\" \
             ORDER BY priority DESC, created_at;",
            &[],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            let mut experiment = record.into_experiment_data(&self.db)?;
            experiment.set_status(&self.db, Status::Running)?;
            experiment.set_assigned_to(&self.db, Some(agent.into()))?;
            Ok(Some((true, experiment)))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Experiments, Status};
    use config::Config;
    use ex::{ExCapLints, ExCrateSelect, ExMode};
    use server::agents::Agents;
    use server::db::Database;
    use server::tokens::Tokens;
    use toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};

    #[test]
    fn test_experiment_creation() {
        let db = Database::temp().unwrap();
        let experiments = Experiments::new(db.clone());

        let api_url = "https://api.github.com/repos/example/example/issues/10";
        let html_url = "https://github.com/example/example/issue/10";

        let config = Config::default();
        experiments
            .create(
                "test".into(),
                &MAIN_TOOLCHAIN,
                &TEST_TOOLCHAIN,
                ExMode::BuildAndTest,
                ExCrateSelect::Demo,
                ExCapLints::Forbid,
                &config,
                Some(api_url),
                Some(html_url),
                Some(10),
                5,
            ).unwrap();

        // Ensure all the data inserted for the experiment is correct
        let ex = experiments.get("test").unwrap().unwrap();
        assert_eq!(ex.experiment.name.as_str(), "test");
        assert_eq!(
            ex.experiment.toolchains,
            [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()]
        );
        assert_eq!(ex.experiment.mode, ExMode::BuildAndTest);
        assert_eq!(ex.experiment.crates, ::ex::demo_list(&config).unwrap());
        assert_eq!(ex.experiment.cap_lints, ExCapLints::Forbid);
        assert_eq!(
            ex.server_data
                .github_issue
                .as_ref()
                .unwrap()
                .api_url
                .as_str(),
            api_url
        );
        assert_eq!(
            ex.server_data
                .github_issue
                .as_ref()
                .unwrap()
                .html_url
                .as_str(),
            html_url
        );
        assert_eq!(ex.server_data.github_issue.as_ref().unwrap().number, 10);
        assert_eq!(ex.server_data.priority, 5);
        assert_eq!(ex.server_data.status, Status::Queued);
        assert!(ex.server_data.assigned_to.is_none());
    }

    #[test]
    fn test_assigning_experiment() {
        let db = Database::temp().unwrap();
        let experiments = Experiments::new(db.clone());

        let mut tokens = Tokens::default();
        tokens.agents.insert("token1".into(), "agent-1".into());
        tokens.agents.insert("token2".into(), "agent-2".into());
        tokens.agents.insert("token3".into(), "agent-3".into());

        // Populate the `agents` table
        let _ = Agents::new(db.clone(), &tokens).unwrap();

        let config = Config::default();
        experiments
            .create(
                "test".into(),
                &MAIN_TOOLCHAIN,
                &TEST_TOOLCHAIN,
                ExMode::BuildAndTest,
                ExCrateSelect::Demo,
                ExCapLints::Forbid,
                &config,
                None,
                None,
                None,
                0,
            ).unwrap();
        experiments
            .create(
                "important".into(),
                &MAIN_TOOLCHAIN,
                &TEST_TOOLCHAIN,
                ExMode::BuildAndTest,
                ExCrateSelect::Demo,
                ExCapLints::Forbid,
                &config,
                None,
                None,
                None,
                10,
            ).unwrap();

        // Test the important experiment is correctly assigned
        let (new, ex) = experiments.next("agent-1").unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.experiment.name.as_str(), "important");
        assert_eq!(ex.server_data.status, Status::Running);
        assert_eq!(ex.server_data.assigned_to.unwrap().as_str(), "agent-1");

        // Test the same experiment is returned to the agent
        let (new, ex) = experiments.next("agent-1").unwrap().unwrap();
        assert!(!new);
        assert_eq!(ex.experiment.name.as_str(), "important");

        // Test the less important experiment is assigned to the next agent
        let (new, ex) = experiments.next("agent-2").unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.experiment.name.as_str(), "test");
        assert_eq!(ex.server_data.status, Status::Running);
        assert_eq!(ex.server_data.assigned_to.unwrap().as_str(), "agent-2");

        // Test no other experiment is available for the other agents
        assert!(experiments.next("agent-3").unwrap().is_none());
    }
}
