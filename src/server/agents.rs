use chrono::Duration;
use chrono::{DateTime, Utc};
use errors::*;
use server::db::{Database, QueryUtils};
use server::experiments::{ExperimentData, Experiments};
use server::tokens::Tokens;
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
    experiment: Option<ExperimentData>,
    last_heartbeat: Option<DateTime<Utc>>,
}

impl Agent {
    fn with_experiment(mut self, db: &Database) -> Result<Self> {
        let experiments = Experiments::new(db.clone());
        self.experiment = experiments.run_by_agent(&self.name)?;
        Ok(self)
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
    pub fn new(db: Database, tokens: &Tokens) -> Result<Self> {
        let agents = Agents { db };
        agents.synchronize(tokens)?;
        Ok(agents)
    }

    fn synchronize(&self, tokens: &Tokens) -> Result<()> {
        self.db.transaction(|trans| {
            let mut real = tokens.agents.values().collect::<HashSet<&String>>();
            for agent in self.all()? {
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

    pub fn all(&self) -> Result<Vec<Agent>> {
        self.db
            .query("SELECT * FROM agents ORDER BY name;", &[], |row| {
                Agent {
                    name: row.get("name"),
                    last_heartbeat: row.get("last_heartbeat"),
                    experiment: None, // Lazy loaded after this
                }
            })?
            .into_iter()
            .map(|agent| agent.with_experiment(&self.db))
            .collect()
    }

    #[cfg(test)]
    fn get(&self, name: &str) -> Result<Option<Agent>> {
        let row = self
            .db
            .get_row("SELECT * FROM agents WHERE name = ?1;", &[&name], |row| {
                Agent {
                    name: row.get("name"),
                    last_heartbeat: row.get("last_heartbeat"),
                    experiment: None, // Lazy loaded after this
                }
            })?;

        Ok(if let Some(agent) = row {
            Some(agent.with_experiment(&self.db)?)
        } else {
            None
        })
    }

    pub fn record_heartbeat(&self, agent: &str) -> Result<()> {
        self.db.execute(
            "UPDATE agents SET last_heartbeat = ?1 WHERE name = ?2;",
            &[&Utc::now(), &agent],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentStatus, Agents};
    use config::Config;
    use ex::{ExCapLints, ExCrateSelect, ExMode};
    use server::db::Database;
    use server::experiments::Experiments;
    use server::tokens::Tokens;
    use toolchain::Toolchain;

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
        assert!(first_heartbeat < agent.last_heartbeat.unwrap());
    }

    #[test]
    fn test_agent_status() {
        let db = Database::temp().unwrap();
        let config = Config::default();
        let experiments = Experiments::new(db.clone());
        let mut tokens = Tokens::default();
        tokens.agents.insert("token".into(), "agent".into());
        let agents = Agents::new(db, &tokens).unwrap();

        // When no heartbeat is recorded, the agent is unreachable
        let agent = agents.get("agent").unwrap().unwrap();
        assert_eq!(agent.status(), AgentStatus::Unreachable);

        // After an heartbeat is recorded, the agent is idle
        agents.record_heartbeat("agent").unwrap();
        let agent = agents.get("agent").unwrap().unwrap();
        assert_eq!(agent.status(), AgentStatus::Idle);

        // Create a new experiment and assign it to the agent
        experiments
            .create(
                "test".into(),
                &Toolchain::Dist("stable".into()),
                &Toolchain::Dist("beta".into()),
                ExMode::BuildAndTest,
                ExCrateSelect::Demo,
                ExCapLints::Forbid,
                &config,
                None,
                None,
                None,
                0,
            )
            .unwrap();
        experiments.next("agent").unwrap();

        // After an experiment is assigned to the agent, the agent is working
        let agent = agents.get("agent").unwrap().unwrap();
        assert_eq!(agent.status(), AgentStatus::Working);
    }
}
