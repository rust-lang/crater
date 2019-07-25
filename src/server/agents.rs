use crate::db::{Database, QueryUtils};
use crate::experiments::{Assignee, Experiment};
use crate::prelude::*;
use crate::server::tokens::Tokens;
use chrono::Duration;
use chrono::{DateTime, Utc};
use std::collections::HashSet;

/// Number of seconds without an heartbeat after an agent should be considered unreachable.
const INACTIVE_AFTER: i64 = 300;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Working,
    Idle,
    Unreachable,
}

pub struct Agent {
    name: String,
    experiment: Option<Experiment>,
    last_heartbeat: Option<DateTime<Utc>>,
    git_revision: Option<String>,
}

impl Agent {
    fn with_experiment(mut self, db: &Database) -> Fallible<Self> {
        self.experiment = Experiment::run_by(db, &Assignee::Agent(self.name.clone()))?;
        Ok(self)
    }

    pub fn git_revision(&self) -> Option<&String> {
        self.git_revision.as_ref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn assigned_experiment(&self) -> Option<&Experiment> {
        self.experiment.as_ref()
    }

    pub fn last_heartbeat(&self) -> Option<&DateTime<Utc>> {
        self.last_heartbeat.as_ref()
    }

    pub fn status(&self) -> AgentStatus {
        if let Some(ref heartbeat) = self.last_heartbeat {
            if Utc::now() - Duration::seconds(INACTIVE_AFTER) < *heartbeat {
                if self.experiment.is_some() {
                    return AgentStatus::Working;
                } else {
                    return AgentStatus::Idle;
                }
            }
        }

        AgentStatus::Unreachable
    }
}

#[derive(Clone)]
pub struct Agents {
    db: Database,
}

impl Agents {
    pub fn new(db: Database, tokens: &Tokens) -> Fallible<Self> {
        let agents = Agents { db };
        agents.synchronize(tokens)?;
        Ok(agents)
    }

    fn synchronize(&self, tokens: &Tokens) -> Fallible<()> {
        self.db.transaction(|trans| {
            let mut real = tokens.agents.values().collect::<HashSet<&String>>();
            for agent in &self.all()? {
                if !real.remove(&agent.name) {
                    trans.execute("DELETE FROM agents WHERE name = ?1;", &[&agent.name])?;
                }
            }

            for missing in &real {
                trans.execute(
                    "INSERT INTO agents (name) VALUES (?1);",
                    &[&missing.as_str()],
                )?;
            }

            Ok(())
        })
    }

    pub fn all(&self) -> Fallible<Vec<Agent>> {
        self.db
            .query("SELECT * FROM agents ORDER BY name;", &[], |row| {
                Agent {
                    name: row.get("name"),
                    last_heartbeat: row.get("last_heartbeat"),
                    git_revision: row.get("git_revision"),
                    experiment: None, // Lazy loaded after this
                }
            })?
            .into_iter()
            .map(|agent| agent.with_experiment(&self.db))
            .collect()
    }

    #[cfg(test)]
    fn get(&self, name: &str) -> Fallible<Option<Agent>> {
        let row = self
            .db
            .get_row("SELECT * FROM agents WHERE name = ?1;", &[&name], |row| {
                Agent {
                    name: row.get("name"),
                    last_heartbeat: row.get("last_heartbeat"),
                    git_revision: row.get("git_revision"),
                    experiment: None, // Lazy loaded after this
                }
            })?;

        Ok(if let Some(agent) = row {
            Some(agent.with_experiment(&self.db)?)
        } else {
            None
        })
    }

    pub fn record_heartbeat(&self, agent: &str) -> Fallible<()> {
        let changes = self.db.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE name = ?2;",
            &[&Utc::now(), &agent],
        )?;
        assert_eq!(changes, 1);

        Ok(())
    }

    pub fn set_git_revision(&self, agent: &str, revision: &str) -> Fallible<()> {
        let changes = self.db.execute(
            "UPDATE agents SET git_revision = ?1 WHERE name = ?2;",
            &[&revision, &agent],
        )?;
        assert_eq!(changes, 1);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentStatus, Agents};
    use crate::actions::{Action, ActionsCtx, CreateExperiment};
    use crate::config::Config;
    use crate::db::Database;
    use crate::experiments::{Assignee, Experiment};
    use crate::server::tokens::Tokens;

    #[test]
    fn test_agents_synchronize() {
        let db = Database::temp().unwrap();
        let agents = Agents::new(db, &Tokens::default()).unwrap();

        let mut tokens = Tokens::default();
        tokens.agents.insert("token1".into(), "agent1".into());
        tokens.agents.insert("token2".into(), "agent2".into());

        agents.synchronize(&tokens).unwrap();
        assert_eq!(
            agents
                .all()
                .unwrap()
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>(),
            vec!["agent1".to_string(), "agent2".to_string()]
        );

        tokens.agents.remove("token1");
        tokens.agents.insert("token3".into(), "agent3".into());

        agents.synchronize(&tokens).unwrap();
        assert_eq!(
            agents
                .all()
                .unwrap()
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>(),
            vec!["agent2".to_string(), "agent3".to_string()]
        );
    }

    #[test]
    fn test_heartbeat_recording() {
        let db = Database::temp().unwrap();
        let mut tokens = Tokens::default();
        tokens.agents.insert("token".into(), "agent".into());
        let agents = Agents::new(db, &tokens).unwrap();

        let agent = agents.get("agent").unwrap().unwrap();
        assert!(agent.last_heartbeat.is_none());

        agents.record_heartbeat("agent").unwrap();

        let agent = agents.get("agent").unwrap().unwrap();
        let first_heartbeat = agent.last_heartbeat.unwrap();

        agents.record_heartbeat("agent").unwrap();

        let agent = agents.get("agent").unwrap().unwrap();
        assert!(first_heartbeat <= agent.last_heartbeat.unwrap());
    }

    #[test]
    fn test_agent_status() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        let mut tokens = Tokens::default();
        tokens.agents.insert("token".into(), "agent".into());
        let agents = Agents::new(db.clone(), &tokens).unwrap();

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // When no heartbeat is recorded, the agent is unreachable
        let agent = agents.get("agent").unwrap().unwrap();
        assert_eq!(agent.status(), AgentStatus::Unreachable);

        // After an heartbeat is recorded, the agent is idle
        agents.record_heartbeat("agent").unwrap();
        let agent = agents.get("agent").unwrap().unwrap();
        assert_eq!(agent.status(), AgentStatus::Idle);

        // Create a new experiment and assign it to the agent
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();
        Experiment::next(&db, &Assignee::Agent("agent".to_string())).unwrap();

        // After an experiment is assigned to the agent, the agent is working
        let agent = agents.get("agent").unwrap().unwrap();
        assert_eq!(agent.status(), AgentStatus::Working);
    }
}
