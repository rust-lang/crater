use crate::agent::Capabilities;
use crate::db::{Database, QueryUtils};
use crate::experiments::{Assignee, Experiment};
use crate::prelude::*;
use crate::server::tokens::Tokens;
use chrono::Duration;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

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
    capabilities: Option<Capabilities>,
}

impl Agent {
    fn with_experiment(mut self, db: &Database) -> Fallible<Self> {
        self.experiment = Experiment::run_by(db, &Assignee::Agent(self.name.clone()))?;
        Ok(self)
    }

    fn with_capabilities(mut self, db: &Database) -> Fallible<Self> {
        self.capabilities = Some(Capabilities::for_agent(db, &self.name)?);
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

    pub fn capabilities(&self) -> Option<&Capabilities> {
        self.capabilities.as_ref()
    }
}

#[derive(Clone)]
pub struct Agents {
    db: Database,
    // worker -> timestamp
    current_workers: Arc<Mutex<HashMap<String, (WorkerInfo, std::time::Instant)>>>,
}

#[derive(Deserialize)]
pub struct WorkerInfo {
    id: String,
}

impl Agents {
    pub fn new(db: Database, tokens: &Tokens) -> Fallible<Self> {
        let agents = Agents {
            db,
            current_workers: Arc::new(Mutex::new(HashMap::new())),
        };
        agents.synchronize(tokens)?;
        Ok(agents)
    }

    pub fn active_worker_count(&self) -> usize {
        let mut guard = self.current_workers.lock().unwrap();
        guard.retain(|_, (_, timestamp)| {
            // It's been 10 minutes since we heard from this worker, drop it from our active list.
            timestamp.elapsed() < std::time::Duration::from_secs(60 * 10)
        });
        guard.len()
    }

    pub fn add_worker(&self, id: WorkerInfo) {
        self.current_workers
            .lock()
            .unwrap()
            .insert(id.id.clone(), (id, std::time::Instant::now()));
    }

    fn synchronize(&self, tokens: &Tokens) -> Fallible<()> {
        self.db.transaction(true, |trans| {
            let mut real = tokens.agents.values().collect::<HashSet<&String>>();
            let current: Vec<String> = self
                .db
                .query("select name from agents;", [], |r| r.get(0))?;

            // If the token is no longer configured, then drop this agent from
            // our list.
            for current_name in current {
                if !real.remove(&current_name) {
                    trans.execute("DELETE FROM agents WHERE name = ?1;", &[&current_name])?;
                }
            }

            // And any *new* agents need to be inserted.
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
            .query("SELECT * FROM agents ORDER BY name;", [], |row| {
                Ok(Agent {
                    name: row.get("name")?,
                    last_heartbeat: row.get("last_heartbeat")?,
                    git_revision: row.get("git_revision")?,

                    // Lazy loaded after this
                    experiment: None,
                    capabilities: None,
                })
            })?
            .into_iter()
            .map(|agent| {
                agent
                    .with_experiment(&self.db)
                    .and_then(|agent| agent.with_capabilities(&self.db))
            })
            .collect()
    }

    #[cfg(test)]
    fn get(&self, name: &str) -> Fallible<Option<Agent>> {
        self.db
            .get_row("SELECT * FROM agents WHERE name = ?1;", [&name], |row| {
                Ok(Agent {
                    name: row.get("name")?,
                    last_heartbeat: row.get("last_heartbeat")?,
                    git_revision: row.get("git_revision")?,

                    // Lazy loaded after this
                    experiment: None,
                    capabilities: None,
                })
            })?
            .map(|agent| agent.with_experiment(&self.db))
            .transpose()?
            .map(|agent| agent.with_capabilities(&self.db))
            .transpose()
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

    pub fn add_capabilities(&self, agent: &str, caps: &Capabilities) -> Fallible<()> {
        const SQL: &str = "INSERT INTO agent_capabilities (agent_name, capability) VALUES (?, ?)";

        self.db.transaction(true, |t| {
            for cap in caps.iter() {
                t.execute_cached(SQL, &[&agent, &cap])?;
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentStatus, Agents};
    use crate::actions::{Action, ActionsCtx, CreateExperiment};
    use crate::agent::Capabilities;
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
        let (_new, ex) = Experiment::next(&db, &Assignee::Agent("agent".to_string()))
            .unwrap()
            .unwrap();
        ex.get_uncompleted_crates(&db, None).unwrap();

        // After an experiment is assigned to the agent, the agent is working
        let agent = agents.get("agent").unwrap().unwrap();
        assert_eq!(agent.status(), AgentStatus::Working);
    }

    #[test]
    fn test_agent_capabilities() {
        let db = Database::temp().unwrap();

        let mut tokens = Tokens::default();
        tokens.agents.insert("token".into(), "agent".into());
        let agents = Agents::new(db.clone(), &tokens).unwrap();

        // Insert capabilities into database
        let caps = Capabilities::new(&["linux", "big-hard-drive"]);
        agents.add_capabilities("agent", &caps).unwrap();

        // Ensure that capabilities are preserved across a round trip to the database.
        let caps_from_db = Capabilities::for_agent(&db, "agent").unwrap();
        assert!(caps.iter().eq(caps_from_db.iter()));
    }
}
