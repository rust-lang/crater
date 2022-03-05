use crate::config::Config;
use crate::crates::Crate;
use crate::db::{Database, QueryUtils};
use crate::prelude::*;
use crate::results::TestResult;
use crate::toolchain::Toolchain;
use crate::utils;
use chrono::{DateTime, Utc};
use rusqlite::Row;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use url::Url;

//sqlite limit is ignored if the expression evaluates to a negative value
static FULL_LIST: i32 = -1;
static SQL_VARIABLE_LIMIT: usize = 500;

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

string_enum!(pub enum CapLints {
    Allow => "allow",
    Warn => "warn",
    Deny => "deny",
    Forbid => "forbid",
});

const SMALL_RANDOM_COUNT: u32 = 20;

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum CrateSelect {
    Full,
    Demo,
    Top(u32),
    Local,
    Dummy,
    Random(u32),
    List(HashSet<String>),
}

from_into_string!(CrateSelect);

impl FromStr for CrateSelect {
    type Err = failure::Error;

    fn from_str(s: &str) -> failure::Fallible<Self> {
        let ret = match s {
            s if s.starts_with("top-") => {
                let n: u32 = s["top-".len()..].parse()?;
                CrateSelect::Top(n)
            }

            "small-random" => CrateSelect::Random(SMALL_RANDOM_COUNT),
            s if s.starts_with("random-") => {
                let n: u32 = s["random-".len()..].parse()?;
                CrateSelect::Random(n)
            }

            s if s.starts_with("list:") => {
                let list = s["list:".len()..]
                    .split(',')
                    .map(|s| s.to_owned())
                    .collect();

                CrateSelect::List(list)
            }

            "full" => CrateSelect::Full,
            "demo" => CrateSelect::Demo,
            "local" => CrateSelect::Local,
            "dummy" => CrateSelect::Dummy,
            s => bail!("invalid CrateSelect: {}", s),
        };

        Ok(ret)
    }
}

impl fmt::Display for CrateSelect {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CrateSelect::Full => write!(f, "full"),
            CrateSelect::Demo => write!(f, "demo"),
            CrateSelect::Dummy => write!(f, "dummy"),
            CrateSelect::Top(n) => write!(f, "top-{}", n),
            CrateSelect::Local => write!(f, "local"),
            CrateSelect::Random(n) => write!(f, "random-{}", n),
            CrateSelect::List(list) => {
                let mut first = true;
                write!(f, "list:")?;

                for krate in list {
                    if !first {
                        write!(f, ",")?;
                    }

                    write!(f, "{}", krate)?;
                    first = false;
                }

                Ok(())
            }
        }
    }
}

impl CrateSelect {
    fn from_newline_separated_list(s: &str) -> Fallible<CrateSelect> {
        if s.contains(',') {
            bail!("Crate identifiers must not contain a comma");
        }

        let crates = s.split_whitespace().map(|s| s.to_owned()).collect();
        Ok(CrateSelect::List(crates))
    }
}

/// Either a `CrateSelect` or `Url` pointing to a list of crates.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum DeferredCrateSelect {
    Direct(CrateSelect),
    Indirect(Url),
}

impl From<CrateSelect> for DeferredCrateSelect {
    fn from(v: CrateSelect) -> Self {
        DeferredCrateSelect::Direct(v)
    }
}

impl DeferredCrateSelect {
    pub fn resolve(self) -> Fallible<CrateSelect> {
        let url = match self {
            DeferredCrateSelect::Direct(v) => return Ok(v),
            DeferredCrateSelect::Indirect(url) => url,
        };

        let body = utils::http::get_sync(url.as_str())?.text()?;
        CrateSelect::from_newline_separated_list(&body)
    }
}

impl FromStr for DeferredCrateSelect {
    type Err = failure::Error;

    fn from_str(input: &str) -> Fallible<Self> {
        if input.starts_with("https://") || input.starts_with("http://") {
            Ok(DeferredCrateSelect::Indirect(input.parse()?))
        } else {
            Ok(DeferredCrateSelect::Direct(input.parse()?))
        }
    }
}

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
    pub requirement: Option<String>,
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
            "select * from experiments where name in ( \
                select experiment from experiment_crates \
                    where status = ?2 and skipped = 0 and assigned_to = ?1 and \
                    experiment in (select name from experiments where status = ?2)) \
            limit 1",
            &[&assignee.to_string(), Status::Running.to_str()],
            |r| ExperimentDBRecord::from_row(r),
        )?;

        if let Some(record) = record {
            Ok(Some(record.into_experiment()?))
        } else {
            Ok(None)
        }
    }

    // Returns the first experiment which has all results ready (and so can
    // produce a complete report). However, the experiment should not be
    // *completed* yet. Note that this may return an experiment which has had
    // report generation already start.
    pub fn ready_for_report(db: &Database) -> Fallible<Option<Experiment>> {
        let unfinished = Self::unfinished(db)?;
        for ex in unfinished {
            if ex.status == Status::ReportFailed {
                // Skip experiments whose report failed to generate. This avoids
                // constantly retrying reports (and posting a message each time
                // about the attempt); the retry-report command can override the
                // failure state. In practice we rarely *fail* to generate
                // reports in a clean way (instead OOMing or panicking, in which
                // case it is fine to automatically retry the report, as we've
                // not posted anything on GitHub -- it may be a problem from a
                // performance perspective but no more than that).
                continue;
            }
            let (completed, all) = ex.raw_progress(db)?;
            if completed == all {
                return Ok(Some(ex));
            }
        }

        Ok(None)
    }

    pub fn find_next(db: &Database, assignee: &Assignee) -> Fallible<Option<Experiment>> {
        // Avoid assigning two experiments to the same agent
        if let Some(experiment) = Experiment::run_by(db, assignee)? {
            return Ok(Some(experiment));
        }

        // Get an experiment whose requirements are met by this agent, preferring (in order of
        // importance):
        //    - experiments that were explicitly assigned to us.
        //    - distributed experiments.
        //    - experiments with a higher priority.
        //    - older experiments.
        Experiment::next_inner(db, Some(assignee), assignee)
            .and_then(|ex| {
                ex.map_or_else(
                    || Experiment::next_inner(db, Some(&Assignee::Distributed), assignee),
                    |exp| Ok(Some(exp)),
                )
            })
            .and_then(|ex| {
                ex.map_or_else(
                    || Experiment::next_inner(db, None, assignee),
                    |exp| Ok(Some(exp)),
                )
            })
    }

    pub fn next(db: &Database, assignee: &Assignee) -> Fallible<Option<(bool, Experiment)>> {
        Self::find_next(db, assignee).and_then(|ex| Self::assign_experiment(db, ex))
    }
    pub fn has_next(db: &Database, assignee: &Assignee) -> Fallible<bool> {
        Ok(Self::find_next(db, assignee)?.is_some())
    }

    fn assign_experiment(
        db: &Database,
        ex: Option<Experiment>,
    ) -> Fallible<Option<(bool, Experiment)>> {
        if let Some(mut experiment) = ex {
            let new_ex = experiment.status != Status::Running;
            if new_ex {
                experiment.set_status(db, Status::Running)?;
                // If this experiment was not assigned to a specific agent make it distributed
                experiment.set_assigned_to(
                    db,
                    experiment
                        .assigned_to
                        .clone()
                        .or(Some(Assignee::Distributed))
                        .as_ref(),
                )?;
            }
            return Ok(Some((new_ex, experiment)));
        }
        Ok(None)
    }

    //CLI query is only partially implemented and is therefore preceded by "unimplemented!"
    #[allow(unreachable_code)]
    fn next_inner(
        db: &Database,
        assignee: Option<&Assignee>,
        agent: &Assignee,
    ) -> Fallible<Option<Experiment>> {
        let agent_name = if let Assignee::Agent(agent_name) = agent {
            agent_name.to_string()
        } else {
            unimplemented!("experiment requirements are not respected when assigning to CLI");
        };

        let (query, params) = if let Some(assignee) = assignee {
            match assignee {
                Assignee::Distributed | Assignee::Agent(_) => {
                    const AGENT_QUERY: &str = r#"
                        SELECT *
                        FROM   experiments ex
                        WHERE  ( ex.status = "queued"
                                OR ( status = "running"
                                            AND ( SELECT COUNT (*)
                                                  FROM  experiment_crates ex_crates
                                                  WHERE ex_crates.experiment = ex.name
                                                          AND ( status = "queued")
                                                          AND ( skipped = 0)
                                                  > 0 ) ) )
                                AND ( ex.assigned_to = ?1 )
                                AND ( ex.requirement IS NULL
                                    OR ex.requirement IN (  SELECT capability
                                                            FROM   agent_capabilities
                                                            WHERE  agent_name = ?2) )
                        ORDER  BY ex.priority DESC,
                                  ex.created_at
                        LIMIT  1;
                    "#;

                    (AGENT_QUERY, vec![assignee.to_string(), agent_name])
                }
                // FIXME: We don't respect experiment requirements when assigning experiments to the
                // CLI. We need to decide what capabilities the CLI should have first.
                _ => {
                    unimplemented!(
                        "experiment requirements are not respected when assigning to CLI"
                    );
                    const CLI_QUERY: &str = r#"
                        SELECT     *
                        FROM       experiments ex
                        WHERE      ( ex.status = "queued"
                                        OR ( status = "running"
                                            AND ( SELECT COUNT (*)
                                                  FROM  experiment_crates ex_crates
                                                  WHERE ex_crates.experiment = ex.name
                                                          AND ( status = "queued")
                                                          AND ( skipped = 0)
                                                  > 0 ) ) )
                                   AND ( ex.assigned_to IS NULL OR ex.assigned_to = ?1 )
                        ORDER BY   ex.assigned_to IS NULL,
                                   ex.priority DESC,
                                   ex.created_at
                        LIMIT 1;
                    "#;

                    (CLI_QUERY, vec![assignee.to_string()])
                }
            }
        } else {
            const AGENT_UNASSIGNED_QUERY: &str = r#"
                SELECT *
                FROM   experiments ex
                WHERE  ( ex.status = "queued"
                        OR ( status = "running"
                                            AND ( SELECT COUNT (*)
                                                  FROM  experiment_crates ex_crates
                                                  WHERE ex_crates.experiment = ex.name
                                                          AND ( status = "queued")
                                                          AND ( skipped = 0)
                                                  > 0 ) ) )
                        AND ( ex.assigned_to IS NULL )
                        AND ( ex.requirement IS NULL
                            OR ex.requirement IN (  SELECT capability
                                                    FROM   agent_capabilities
                                                    WHERE  agent_name = ?1) )
                ORDER  BY ex.priority DESC,
                          ex.created_at
                LIMIT  1;
            "#;

            (AGENT_UNASSIGNED_QUERY, vec![agent_name])
        };

        if let Some(record) = db.get_row(query, rusqlite::params_from_iter(params.iter()), |r| {
            ExperimentDBRecord::from_row(r)
        })? {
            Ok(Some(record.into_experiment()?))
        } else {
            Ok(None)
        }
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

    pub fn handle_failure(&mut self, db: &Database, agent: &Assignee) -> Fallible<()> {
        // Mark all the running crates from this agent as queued (so that they
        // run again)
        db.execute(
            "
            UPDATE experiment_crates
            SET assigned_to = NULL, status = ?1 \
            WHERE experiment = ?2 AND status = ?3 \
            AND assigned_to = ?4
            ",
            &[
                &Status::Queued.to_string(),
                &self.name,
                &Status::Running.to_string(),
                &agent.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn set_status(&mut self, db: &Database, status: Status) -> Fallible<()> {
        db.execute(
            "UPDATE experiments SET status = ?1 WHERE name = ?2;",
            &[&status.to_str(), &self.name.as_str()],
        )?;

        let now = Utc::now();

        match (self.status, status) {
            // Check if the new status is "running" and there is no starting date
            (_, Status::Running) if self.started_at.is_none() => {
                db.execute(
                    "UPDATE experiments SET started_at = ?1 WHERE name = ?2;",
                    &[&now, &self.name.as_str()],
                )?;
                self.started_at = Some(now);
            }
            // Check if the old status was "running" and there is no completed date
            (Status::Running, new_status)
                if self.completed_at.is_none() && new_status != Status::Failed =>
            {
                db.execute(
                    "UPDATE experiments SET completed_at = ?1 WHERE name = ?2;",
                    &[&now, &self.name.as_str()],
                )?;
                self.completed_at = Some(now);
            }
            // Queue again failed crates
            (Status::Failed, Status::Queued) => {
                db.execute(
                    "UPDATE experiment_crates
                    SET status = ?1 \
                    WHERE experiment = ?2 AND status = ?3
                    ",
                    &[
                        &Status::Queued.to_string(),
                        &self.name,
                        &Status::Failed.to_string(),
                    ],
                )?;
            }
            _ => (),
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

    pub fn get_result_counts(&self, db: &Database) -> Fallible<Vec<(TestResult, u32)>> {
        let results: Vec<(String, u32)> = db.query(
            "SELECT result, COUNT(*) FROM results \
             WHERE experiment = ?1 GROUP BY result;",
            &[&self.name.as_str()],
            |r| Ok((r.get::<_, String>(0)?, r.get(1)?)),
        )?;

        results
            .into_iter()
            .map(|(result, count)| Ok((TestResult::from_str(&result)?, count)))
            .collect()
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
            |r| r.get(0),
        )?
        .into_iter()
        .map(|c: String| c.parse())
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
        let assigned_to = assigned_to.to_string();

        db.transaction(|transaction| {
            //get the first 'limit' queued crates from the experiment crates list
            let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            let crates = transaction
                .query(
                    "SELECT crate FROM experiment_crates WHERE experiment = ?1
                     AND status = ?2 AND skipped = 0 LIMIT ?3;",
                    rusqlite::params![self.name, Status::Queued.to_string(), limit],
                    |r| r.get("crate"),
                )?
                .into_iter()
                .collect::<Vec<String>>();

            crates.iter().for_each(|krate| params.push(krate));
            let params_header: &[&dyn rusqlite::types::ToSql] = &[&assigned_to, &self.name];
            //SQLite cannot handle queries with more than 999 variables
            for params in params.chunks(SQL_VARIABLE_LIMIT) {
                let params = [params_header, params].concat();
                let update_query = &[
                    "
                    UPDATE experiment_crates
                    SET assigned_to = ?1, status = \"running\" \
                    WHERE experiment = ?2
                    AND crate IN ("
                        .to_string(),
                    "?,".repeat(params.len() - 3),
                    "?)".to_string(),
                ]
                .join("");

                //update the status of the previously selected crates to 'Running'
                transaction.execute(update_query, &params)?;
            }
            crates
                .iter()
                .map(|krate| Ok(krate.parse()?))
                .collect::<Fallible<Vec<Crate>>>()
        })
    }

    pub fn get_running_crates(
        &self,
        db: &Database,
        assigned_to: &Assignee,
    ) -> Fallible<Vec<Crate>> {
        db.query(
            "SELECT crate FROM experiment_crates WHERE experiment = ?1 \
             AND status = ?2 AND assigned_to = ?3",
            &[
                &self.name,
                &Status::Running.to_string(),
                &assigned_to.to_string(),
            ],
            |r| r.get(0),
        )?
        .into_iter()
        .map(|c: String| c.parse())
        .collect::<Fallible<Vec<Crate>>>()
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
    requirement: Option<String>,
}

impl ExperimentDBRecord {
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(ExperimentDBRecord {
            name: row.get("name")?,
            mode: row.get("mode")?,
            cap_lints: row.get("cap_lints")?,
            toolchain_start: row.get("toolchain_start")?,
            toolchain_end: row.get("toolchain_end")?,
            priority: row.get("priority")?,
            created_at: row.get("created_at")?,
            started_at: row.get("started_at")?,
            completed_at: row.get("completed_at")?,
            status: row.get("status")?,
            github_issue: row.get("github_issue")?,
            github_issue_url: row.get("github_issue_url")?,
            github_issue_number: row.get("github_issue_number")?,
            assigned_to: row.get("assigned_to")?,
            report_url: row.get("report_url")?,
            ignore_blacklist: row.get("ignore_blacklist")?,
            requirement: row.get("requirement")?,
        })
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
            requirement: self.requirement,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Assignee, AssigneeParseError, CrateSelect, DeferredCrateSelect, Experiment, Status,
    };
    use crate::actions::{Action, ActionsCtx, CreateExperiment};
    use crate::agent::Capabilities;
    use crate::config::Config;
    use crate::db::Database;
    use crate::server::agents::Agents;
    use crate::server::tokens::Tokens;
    use std::collections::HashSet;
    use std::str::FromStr;

    #[test]
    fn test_crate_select_parsing() {
        let demo_crates: HashSet<_> = ["brson/hello-rs", "lazy_static"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let suite = vec![
            ("demo", CrateSelect::Demo),
            ("top-25", CrateSelect::Top(25)),
            ("random-87", CrateSelect::Random(87)),
            ("small-random", CrateSelect::Random(20)),
            (
                "list:brson/hello-rs,lazy_static",
                CrateSelect::List(demo_crates.clone()),
            ),
        ];

        for (s, output) in suite.into_iter() {
            assert_eq!(CrateSelect::from_str(s).unwrap(), output);
            assert_eq!(
                DeferredCrateSelect::from_str(s).unwrap(),
                DeferredCrateSelect::Direct(output),
            );
        }

        assert_eq!(
            DeferredCrateSelect::from_str("http://git.io/Jes7o").unwrap(),
            DeferredCrateSelect::Indirect("http://git.io/Jes7o".parse().unwrap()),
        );

        assert_eq!(
            DeferredCrateSelect::from_str("https://git.io/Jes7o").unwrap(),
            DeferredCrateSelect::Indirect("https://git.io/Jes7o".parse().unwrap()),
        );

        let list = CrateSelect::from_newline_separated_list(
            r"
            brson/hello-rs

            lazy_static",
        )
        .unwrap();

        assert_eq!(list, CrateSelect::List(demo_crates));
    }

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
    fn test_assigning_experiment_with_requirements() {
        let db = Database::temp().unwrap();
        let config = Config::load().unwrap();

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        let mut tokens = Tokens::default();
        tokens.agents.insert("token1".into(), "agent-1".into());
        tokens.agents.insert("token2".into(), "agent-2".into());

        let agent1 = Assignee::Agent("agent-1".to_string());
        let agent2 = Assignee::Agent("agent-2".to_string());

        // Populate the `agents` table
        let agents = Agents::new(db.clone(), &tokens).unwrap();
        agents
            .add_capabilities("agent-1", &Capabilities::new(&["linux"]))
            .unwrap();
        agents
            .add_capabilities(
                "agent-2",
                &Capabilities::new(&["windows", "big-hard-drive"]),
            )
            .unwrap();

        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        let mut windows = CreateExperiment::dummy("windows");
        windows.requirement = Some("windows".to_string());
        windows.apply(&ctx).unwrap();

        // Test that an experiment will not be assigned to an agent without the required
        // capabilities.
        assert!(Experiment::next(&db, &agent1).unwrap().is_none());

        // Test that an experiment with no capabilities can be assigned to any agent.
        CreateExperiment::dummy("no-requirements")
            .apply(&ctx)
            .unwrap();

        let (new, mut ex) = Experiment::next(&db, &agent1).unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.name.as_str(), "no-requirements");
        assert_eq!(ex.status, Status::Running);
        assert_eq!(ex.assigned_to.clone().unwrap(), Assignee::Distributed);

        //Mark the experiment as completed, otherwise agent2 will still pick it
        //as it has uncompleted crates
        ex.set_status(&db, Status::Completed).unwrap();

        // Test that an experiment will be assigned to an agent with the required capabilities.
        let (new, ex) = Experiment::next(&db, &agent2).unwrap().unwrap();
        assert!(new);
        assert_eq!(ex.name.as_str(), "windows");
        assert_eq!(ex.status, Status::Running);
        assert_eq!(ex.assigned_to.unwrap(), Assignee::Distributed);
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
        rustwide::logging::init();

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

    // A failure is handled by re-queueing any running crates for a given agent,
    // to be picked up by the next agent to ask for them.
    #[test]
    fn test_failed_experiment() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);
        let agent1 = Assignee::Agent("agent-1".to_string());

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();
        let mut ex = Experiment::next(&db, &agent1).unwrap().unwrap().1;
        assert!(!ex
            .get_uncompleted_crates(&db, &config, &agent1)
            .unwrap()
            .is_empty());
        ex.handle_failure(&db, &agent1).unwrap();
        assert!(Experiment::next(&db, &agent1).unwrap().is_some());
        assert_eq!(ex.status, Status::Running);
        assert!(!ex
            .get_uncompleted_crates(&db, &config, &agent1)
            .unwrap()
            .is_empty());
    }
}
