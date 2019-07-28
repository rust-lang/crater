use crate::config::Config;
use crate::crates::Crate;
use crate::db::{Database, QueryUtils};
use crate::prelude::*;
use crate::toolchain::Toolchain;
use chrono::{DateTime, Utc};
use rusqlite::Row;
use serde_json;
use std::fmt;
use std::str::FromStr;

//sqlite limit is ignored if the expression evaluates to a negative value
static FULL_LIST: i32 = -1;

string_enum!(pub enum Status {
    Queued => "queued",
    Running => "running",
    NeedsReport => "needs-report",
    Failed => "failed",
    GeneratingReport => "generating-report",
    ReportFailed => "report-failed",
    Completed => "completed",
});

string_enum!(pub enum Mode {
    BuildAndTest => "build-and-test",
    BuildOnly => "build-only",
    CheckOnly => "check-only",
    Clippy => "clippy",
    Rustdoc => "rustdoc",
    UnstableFeatures => "unstable-features",
});

string_enum!(pub enum CrateSelect {
    Full => "full",
    Demo => "demo",
    SmallRandom => "small-random",
    Top100 => "top-100",
    Local => "local",
});

string_enum!(pub enum CapLints {
    Allow => "allow",
    Warn => "warn",
    Deny => "deny",
    Forbid => "forbid",
});

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[derive(Clone, Serialize, Deserialize)]
pub enum Assignee {
    Agent(String),
    Distributed,
    CLI,
}

impl fmt::Display for Assignee {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Assignee::Agent(ref name) => write!(f, "agent:{}", name),
            Assignee::Distributed => write!(f, "distributed"),
            Assignee::CLI => write!(f, "cli"),
        }
    }
}

#[derive(Debug, Fail)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum AssigneeParseError {
    #[fail(display = "the assignee is empty")]
    Empty,
    #[fail(display = "unexpected assignee payload")]
    UnexpectedPayload,
    #[fail(display = "invalid assignee kind: {}", _0)]
    InvalidKind(String),
}

impl FromStr for Assignee {
    type Err = AssigneeParseError;

    fn from_str(input: &str) -> Result<Self, AssigneeParseError> {
        if input.trim().is_empty() {
            return Err(AssigneeParseError::Empty);
        }

        let mut split = input.splitn(2, ':');
        let kind = split.next().ok_or(AssigneeParseError::Empty)?;

        match kind {
            "agent" => {
                let name = split.next().ok_or(AssigneeParseError::Empty)?;
                if name.trim().is_empty() {
                    return Err(AssigneeParseError::Empty);
                }

                Ok(Assignee::Agent(name.to_string()))
            }
            "cli" => {
                if split.next().is_some() {
                    return Err(AssigneeParseError::UnexpectedPayload);
                }

                Ok(Assignee::CLI)
            }
            "distributed" => {
                if split.next().is_some() {
                    return Err(AssigneeParseError::UnexpectedPayload);
                }

                Ok(Assignee::Distributed)
            }
            invalid => Err(AssigneeParseError::InvalidKind(invalid.into())),
        }
    }
}

impl Assignee {
    pub fn get_name(&self) -> &str {
        match self {
            Assignee::Agent(ref name) => name,
            Assignee::Distributed => "distributed",
            Assignee::CLI => "cli",
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GitHubIssue {
    pub api_url: String,
    pub html_url: String,
    pub number: i32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Experiment {
    pub name: String,
    pub toolchains: [Toolchain; 2],
    pub mode: Mode,
    pub cap_lints: CapLints,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub github_issue: Option<GitHubIssue>,
    pub status: Status,
    pub assigned_to: Option<Assignee>,
    pub report_url: Option<String>,
    pub ignore_blacklist: bool,
}

impl Experiment {
    pub fn exists(db: &Database, name: &str) -> Fallible<bool> {
        Ok(db.exists("SELECT rowid FROM experiments WHERE name = ?1;", &[&name])?)
    }

    pub fn unfinished(db: &Database) -> Fallible<Vec<Experiment>> {
        let records = db.query(
            "SELECT * FROM experiments WHERE status != ?1 ORDER BY priority DESC, created_at;",
            &[&Status::Completed.to_str()],
            |r| ExperimentDBRecord::from_row(r),
        )?;
        records
            .into_iter()
            .map(|record| record.into_experiment())
            .collect::<Fallible<_>>()
    }

    pub fn run_by(db: &Database, assignee: &Assignee) -> Fallible<Option<Experiment>> {
        let record = db.get_row(
            "SELECT * FROM experiments \
             INNER JOIN experiment_crates ON experiment_crates.experiment \
             = experiments.name WHERE experiment_crates.assigned_to = ?1 \
             AND experiment_crates.status = ?2  AND experiment_crates.skipped = 0 LIMIT 1",
            &[&assignee.to_string(), &Status::Running.to_str()],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            Ok(Some(record.into_experiment()?))
        } else {
            Ok(None)
        }
    }

    pub fn first_by_status(db: &Database, status: Status) -> Fallible<Option<Experiment>> {
        let record = db.get_row(
            "SELECT * FROM experiments \
             WHERE status = ?1 \
             ORDER BY priority DESC, created_at;",
            &[&status.to_str()],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            Ok(Some(record.into_experiment()?))
        } else {
            Ok(None)
        }
    }

    pub fn next(db: &Database, assignee: &Assignee) -> Fallible<Option<(bool, Experiment)>> {
        // Avoid assigning two experiments to the same agent
        if let Some(experiment) = Experiment::run_by(db, assignee)? {
            return Ok(Some((false, experiment)));
        }

        Experiment::next_inner(db, Some(assignee))
            .and_then(|ex| {
                ex.map_or_else(
                    || Experiment::next_inner(db, Some(&Assignee::Distributed)),
                    |exp| Ok(Some(exp)),
                )
            })
            .and_then(|ex| ex.map_or_else(|| Experiment::next_inner(db, None), |exp| Ok(Some(exp))))
    }

    fn next_inner(
        db: &Database,
        assignee: Option<&Assignee>,
    ) -> Fallible<Option<(bool, Experiment)>> {
        let record = if let Some(assigned_to) = assignee {
            db.get_row(
                "SELECT * FROM experiments WHERE (status = ?1 \
                 OR (status = ?2   AND (SELECT COUNT(*) FROM experiment_crates \
                 WHERE experiment = experiments.name  AND status = ?1 AND skipped = 0) > 0)) \
                 AND assigned_to = ?3 \
                 ORDER BY priority DESC, created_at;",
                &[
                    &Status::Queued.to_string(),
                    &Status::Running.to_string(),
                    &assigned_to.to_string(),
                ],
                |r| ExperimentDBRecord::from_row(r),
            )?
        } else {
            db.get_row(
                "SELECT * FROM experiments WHERE (status = ?1 \
                 OR (status = ?2   AND (SELECT COUNT(*) FROM experiment_crates \
                 WHERE experiment = experiments.name  AND status = ?1 AND skipped = 0) > 0)) \
                 AND assigned_to IS NULL \
                 ORDER BY priority DESC, created_at;",
                &[&Status::Queued.to_string(), &Status::Running.to_string()],
                |r| ExperimentDBRecord::from_row(r),
            )?
        };

        if let Some(record) = record {
            let mut experiment = record.into_experiment()?;
            let new_ex = experiment.status != Status::Running;
            experiment.set_status(&db, Status::Running)?;
            experiment.set_assigned_to(&db, assignee.or(Some(&Assignee::Distributed)))?;
            return Ok(Some((new_ex, experiment)));
        }
        Ok(None)
    }

    pub fn get(db: &Database, name: &str) -> Fallible<Option<Experiment>> {
        let record = db.get_row(
            "SELECT * FROM experiments WHERE name = ?1;",
            &[&name],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            Ok(Some(record.into_experiment()?))
        } else {
            Ok(None)
        }
    }

    pub fn set_status(&mut self, db: &Database, status: Status) -> Fallible<()> {
        db.execute(
            "UPDATE experiments SET status = ?1 WHERE name = ?2;",
            &[&status.to_str(), &self.name.as_str()],
        )?;

        let now = Utc::now();

        // Check if the new status is "running" and there is no starting date
        if status == Status::Running && self.started_at.is_none() {
            db.execute(
                "UPDATE experiments SET started_at = ?1 WHERE name = ?2;",
                &[&now, &self.name.as_str()],
            )?;
            self.started_at = Some(now);
        // Check if the old status was "running" and there is no completed date
        } else if self.status == Status::Running
            && self.completed_at.is_none()
            && status != Status::Failed
        {
            db.execute(
                "UPDATE experiments SET completed_at = ?1 WHERE name = ?2;",
                &[&now, &self.name.as_str()],
            )?;
            self.completed_at = Some(now);
        }

        self.status = status;
        Ok(())
    }

    pub fn set_assigned_to(
        &mut self,
        db: &Database,
        assigned_to: Option<&Assignee>,
    ) -> Fallible<()> {
        db.execute(
            "UPDATE experiments SET assigned_to = ?1 WHERE name = ?2;",
            &[&assigned_to.map(|a| a.to_string()), &self.name.as_str()],
        )?;

        self.assigned_to = assigned_to.cloned();
        Ok(())
    }

    pub fn set_report_url(&mut self, db: &Database, url: &str) -> Fallible<()> {
        db.execute(
            "UPDATE experiments SET report_url = ?1 WHERE name = ?2;",
            &[&url, &self.name.as_str()],
        )?;
        self.report_url = Some(url.to_string());
        Ok(())
    }

    pub fn raw_progress(&self, db: &Database) -> Fallible<(u32, u32)> {
        let results_len: u32 = db
            .get_row(
                "SELECT COUNT(*) AS count FROM results WHERE experiment = ?1;",
                &[&self.name.as_str()],
                |r| r.get("count"),
            )?
            .unwrap();

        let crates_len: u32 = db
            .get_row(
                "SELECT COUNT(*) AS count FROM experiment_crates \
                 WHERE experiment = ?1 AND skipped = 0;",
                &[&self.name.as_str()],
                |r| r.get("count"),
            )?
            .unwrap();

        Ok((results_len, crates_len * 2))
    }

    pub fn progress(&self, db: &Database) -> Fallible<u8> {
        let (results_len, crates_len) = self.raw_progress(db)?;

        if crates_len != 0 {
            Ok((results_len as f32 * 100.0 / crates_len as f32).ceil() as u8)
        } else {
            Ok(0)
        }
    }

    pub fn get_crates(&self, db: &Database) -> Fallible<Vec<Crate>> {
        db.query(
            "SELECT crate FROM experiment_crates WHERE experiment = ?1;",
            &[&self.name],
            |r| {
                let value: String = r.get("crate");
                Ok(serde_json::from_str(&value)?)
            },
        )?
        .into_iter()
        .collect::<Fallible<Vec<Crate>>>()
    }

    fn crate_list_size(&self, config: &Config) -> i32 {
        match self.assigned_to {
            //if experiment is distributed return chunk
            Some(Assignee::Distributed) => config.chunk_size(),
            //if experiment is assigned to specific agent return all the crates
            _ => FULL_LIST,
        }
    }

    pub fn get_uncompleted_crates(
        &self,
        db: &Database,
        config: &Config,
        assigned_to: &Assignee,
    ) -> Fallible<Vec<Crate>> {
        let limit = self.crate_list_size(config);

        //get the first 'limit' queued crates from the experiment crates list
        db.transaction(|transaction| {
            let crates = transaction.query(
                "SELECT crate FROM experiment_crates WHERE experiment = ?1
                AND status = ?2 AND skipped = 0 LIMIT ?3;",
                &[&self.name, &Status::Queued.to_string(), &limit],
                |r| {
                    let value: String = r.get("crate");
                    Ok(serde_json::from_str(&value)?)
                },
            )?
            .into_iter()
            .collect::<Fallible<Vec<Crate>>>();

            //update the status of the previously selected crates to 'Running'
            transaction.execute(
                "UPDATE experiment_crates SET assigned_to = ?1, status = ?2 \
                WHERE experiment = ?3 AND crate IN (SELECT crate FROM experiment_crates WHERE experiment = ?3
                AND status = ?4 AND skipped = 0 LIMIT ?5) ",
                &[
                    &assigned_to.to_string(),
                    &Status::Running.to_string(),
                    &self.name,
                    &Status::Queued.to_string(),
                    &limit,
                ],
            )?;

            crates
        })
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
    ignore_blacklist: bool,
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
            ignore_blacklist: row.get("ignore_blacklist"),
        }
    }

    fn into_experiment(self) -> Fallible<Experiment> {
        Ok(Experiment {
            name: self.name,
            toolchains: [self.toolchain_start.parse()?, self.toolchain_end.parse()?],
            cap_lints: self.cap_lints.parse()?,
            mode: self.mode.parse()?,
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
            assigned_to: if let Some(assignee) = self.assigned_to {
                Some(assignee.parse()?)
            } else {
                None
            },
            status: self.status.parse()?,
            report_url: self.report_url,
            ignore_blacklist: self.ignore_blacklist,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Assignee, AssigneeParseError, Experiment, Status};
    use crate::actions::{Action, ActionsCtx, CreateExperiment};
    use crate::config::Config;
    use crate::db::Database;
    use crate::server::agents::Agents;
    use crate::server::tokens::Tokens;
    use std::str::FromStr;

    #[test]
    fn test_assignee_parsing() {
        assert_eq!(
            Assignee::Agent("foo".to_string()).to_string().as_str(),
            "agent:foo"
        );
        assert_eq!(
            Assignee::from_str("agent:foo").unwrap(),
            Assignee::Agent("foo".to_string())
        );

        assert_eq!(Assignee::CLI.to_string().as_str(), "cli");
        assert_eq!(Assignee::from_str("cli").unwrap(), Assignee::CLI);

        for empty in &["", "agent:"] {
            let err = Assignee::from_str(empty).unwrap_err();
            assert_eq!(err, AssigneeParseError::Empty);
        }

        let err = Assignee::from_str("foo").unwrap_err();
        assert_eq!(err, AssigneeParseError::InvalidKind("foo".into()));

        for invalid in &["cli:", "cli:foo"] {
            let err = Assignee::from_str(invalid).unwrap_err();
            assert_eq!(err, AssigneeParseError::UnexpectedPayload);
        }
    }

    #[test]
    fn test_assigning_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::load().unwrap();

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        let mut tokens = Tokens::default();
        tokens.agents.insert("token1".into(), "agent-1".into());
        tokens.agents.insert("token2".into(), "agent-2".into());
        tokens.agents.insert("token3".into(), "agent-3".into());

        let agent1 = Assignee::Agent("agent-1".to_string());
        let agent2 = Assignee::Agent("agent-2".to_string());
        let agent3 = Assignee::Agent("agent-3".to_string());

        // Populate the `agents` table
        let _ = Agents::new(db.clone(), &tokens).unwrap();

        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        CreateExperiment::dummy("test").apply(&ctx).unwrap();

        let mut create_important = CreateExperiment::dummy("important");
        create_important.priority = 10;
        create_important.apply(&ctx).unwrap();

        // Test the important experiment is correctly assigned
        let (new, ex) = Experiment::next(&db, &agent1).unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.name.as_str(), "important");
        assert_eq!(ex.status, Status::Running);
        assert_eq!(ex.assigned_to.unwrap(), Assignee::Distributed);

        // Test the same experiment is returned to the agent
        let (new, mut ex) = Experiment::next(&db, &agent1).unwrap().unwrap();
        assert!(!new);
        assert_eq!(ex.name.as_str(), "important");

        //Mark the experiment as completed, otherwise agent2 will still pick it as has uncompleted crates
        ex.set_status(&db, Status::Completed).unwrap();

        // Test the less important experiment is assigned to the next agent
        let (new, mut ex) = Experiment::next(&db, &agent2).unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.name.as_str(), "test");
        assert_eq!(ex.status, Status::Running);
        assert_eq!(ex.assigned_to.clone().unwrap(), Assignee::Distributed);

        //Mark the experiment as completed, otherwise agent3 will still pick it as has uncompleted crates
        ex.set_status(&db, Status::Completed).unwrap();

        // Test no other experiment is available for the other agents
        assert!(Experiment::next(&db, &agent3).unwrap().is_none());
    }

    #[test]
    fn test_assigning_experiment_with_preassigned_agent() {
        let db = Database::temp().unwrap();
        let config = Config::load().unwrap();

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        let mut tokens = Tokens::default();
        tokens.agents.insert("token1".into(), "agent-1".into());
        tokens.agents.insert("token2".into(), "agent-2".into());

        let agent1 = Assignee::Agent("agent-1".to_string());
        let agent2 = Assignee::Agent("agent-2".to_string());

        // Populate the `agents` table
        let _ = Agents::new(db.clone(), &tokens).unwrap();

        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        let mut create_assigned = CreateExperiment::dummy("assigned");
        create_assigned.assign = Some(agent1.clone());
        create_assigned.apply(&ctx).unwrap();

        let mut create_important = CreateExperiment::dummy("important");
        create_important.priority = 10;
        create_important.apply(&ctx).unwrap();

        // Try to get an experiment for agent 1, it should pick 'assigned' even if 'important' has
        // an higher priority.
        let (new, ex) = Experiment::next(&db, &agent1).unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.assigned_to.unwrap(), agent1);
        assert_eq!(ex.name.as_str(), "assigned");

        // Then the 'important' experiment will be picked by agent 2
        let (new, ex) = Experiment::next(&db, &agent2).unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.assigned_to.unwrap(), Assignee::Distributed);
        assert_eq!(ex.name.as_str(), "important");
    }

    #[test]
    fn test_full_completed_crates() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();
        let ex = Experiment::get(&db, "dummy").unwrap().unwrap();
        let crates = ex
            .get_uncompleted_crates(&db, &config, &Assignee::CLI)
            .unwrap();
        // Assert the whole list is returned
        assert_eq!(crates.len(), ex.get_crates(&db).unwrap().len());

        // Test already completed crates does not show up again
        let uncompleted_crates = ex
            .get_uncompleted_crates(&db, &config, &Assignee::CLI)
            .unwrap();
        assert_eq!(uncompleted_crates.len(), 0);
    }
}
