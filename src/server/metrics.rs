use crate::db::Database;
use crate::experiments::{Assignee, Experiment};
use crate::prelude::*;
use crate::server::agents::Agent;
use prometheus::proto::{Metric, MetricFamily};
use prometheus::{IntCounterVec, IntGaugeVec, __register_counter_vec, __register_gauge_vec};
const JOBS_METRIC: &str = "crater_completed_jobs_total";
const AGENT_WORK_METRIC: &str = "crater_agent_supposed_to_work";

#[derive(Clone)]
pub struct Metrics {
    crater_completed_jobs_total: IntCounterVec,
    crater_work_status: IntGaugeVec,
}

impl Metrics {
    pub fn new() -> Fallible<Self> {
        let jobs_opts = prometheus::opts!(JOBS_METRIC, "total completed jobs");
        let crater_completed_jobs_total =
            prometheus::register_int_counter_vec!(jobs_opts, &["agent", "experiment"])?;
        let agent_opts = prometheus::opts!(AGENT_WORK_METRIC, "is agent supposed to work");
        let crater_work_status = prometheus::register_int_gauge_vec!(agent_opts, &["agent"])?;

        Ok(Metrics {
            crater_completed_jobs_total,
            crater_work_status,
        })
    }

    pub fn record_completed_jobs(&self, agent: &str, experiment: &str, amount: i64) {
        self.crater_completed_jobs_total
            .with_label_values(&[&agent, &experiment])
            .inc_by(amount);
    }

    fn get_metric_by_name(name: &str) -> Option<MetricFamily> {
        let families = prometheus::gather();
        families.into_iter().find(|fam| fam.get_name() == name)
    }

    fn get_label_by_name<'a>(metric: &'a Metric, label: &str) -> Option<&'a str> {
        metric
            .get_label()
            .iter()
            .find(|lab| lab.get_name() == label)
            .map(|lab| lab.get_value())
    }

    fn remove_experiment_jobs(&self, experiment: &str) -> Fallible<()> {
        if let Some(metric) = Self::get_metric_by_name(JOBS_METRIC) {
            let agents = metric
                .get_metric()
                .iter()
                .filter(|met| Self::get_label_by_name(met, "experiment").unwrap() == experiment)
                .map(|met| Self::get_label_by_name(met, "agent").unwrap())
                .collect::<Vec<&str>>();

            for agent in agents.iter() {
                self.crater_completed_jobs_total
                    .remove_label_values(&[agent, &experiment])?;
            }
        }

        Ok(())
    }

    pub fn update_agent_status(&self, db: &Database, agents: &[&Agent]) -> Fallible<()> {
        self.crater_work_status.reset();

        for agent in agents {
            let assignee = Assignee::Agent(agent.name().to_string());
            let has_work = Experiment::has_next(db, &assignee)?;

            self.crater_work_status
                .with_label_values(&[agent.name()])
                .set(has_work as i64);
        }

        Ok(())
    }

    pub fn on_complete_experiment(&self, experiment: &str) -> Fallible<()> {
        self.remove_experiment_jobs(experiment)
    }
}

#[cfg(test)]
mod tests {
    use super::{Metrics, AGENT_WORK_METRIC, JOBS_METRIC};
    use crate::actions::{Action, ActionsCtx, CreateExperiment, EditExperiment};
    use crate::config::Config;
    use crate::db::Database;
    use crate::experiments::{Assignee, Experiment};
    use crate::server::agents::{Agent, Agents};
    use crate::server::tokens::Tokens;
    use lazy_static::lazy_static;
    use prometheus::proto::MetricFamily;

    lazy_static! {
        static ref METRICS: Metrics = Metrics::new().unwrap();
    }

    fn test_experiment_presence(metric: &MetricFamily, experiment: &str) -> bool {
        metric
            .get_metric()
            .iter()
            .any(|met| Metrics::get_label_by_name(met, "experiment").unwrap() == experiment)
    }

    #[test]
    fn test_on_complete_experiment() {
        let ex1 = "pr-0";
        let ex2 = "pr-1";
        let agent1 = "agent-1";
        let agent2 = "agent-2";

        METRICS.record_completed_jobs(agent1, ex1, 1);
        METRICS.record_completed_jobs(agent2, ex1, 1);
        METRICS.record_completed_jobs(agent2, ex2, 1);

        //test metrics are correctly registered
        let jobs = Metrics::get_metric_by_name(JOBS_METRIC).unwrap();
        assert!(test_experiment_presence(&jobs, ex1));
        assert!(test_experiment_presence(&jobs, ex2));

        //test metrics are correctly removed when an experiment is completed
        METRICS.on_complete_experiment(ex1).unwrap();

        let jobs = Metrics::get_metric_by_name(JOBS_METRIC).unwrap();
        assert!(!test_experiment_presence(&jobs, ex1));
        assert!(test_experiment_presence(&jobs, ex2));

        //test nothing bad happens when a specific
        //experiment is executed by a subset of the agents
        METRICS.on_complete_experiment(ex2).unwrap();
    }

    fn supposed_to_work(metric: &MetricFamily, agent_filter: Option<&str>) -> bool {
        metric
            .get_metric()
            .iter()
            .filter(|met| {
                agent_filter.map_or_else(
                    || true,
                    |agent| Metrics::get_label_by_name(met, "agent").unwrap() == agent,
                )
            })
            .all(|met| met.get_gauge().get_value() as u64 == 1)
    }

    #[test]
    fn test_lazy_agents() {
        let agent1 = "agent-1";
        let agent2 = "agent-2";

        let db = Database::temp().unwrap();

        let mut tokens = Tokens::default();
        tokens.agents.insert("token1".into(), agent1.into());
        tokens.agents.insert("token2".into(), agent2.into());
        let agents = Agents::new(db.clone(), &tokens).unwrap();

        for agent in agents.all().unwrap().iter() {
            agents.record_heartbeat(agent.name()).unwrap();
        }

        let agent_list = agents.all().unwrap();
        let agent_list_ref = agent_list.iter().collect::<Vec<&Agent>>();

        METRICS.update_agent_status(&db, &agent_list_ref).unwrap();

        // Nothing to do
        let status = Metrics::get_metric_by_name(AGENT_WORK_METRIC).unwrap();
        assert!(!supposed_to_work(&status, None));

        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);
        let assignee = Assignee::Agent(agent1.to_string());
        crate::crates::lists::setup_test_lists(&db, &config).unwrap();
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();

        METRICS.update_agent_status(&db, &agent_list_ref).unwrap();

        // Experiment is queued, all the agents should have work to do
        let status = Metrics::get_metric_by_name(AGENT_WORK_METRIC).unwrap();
        assert!(supposed_to_work(&status, None));

        // Assign experiment to agent-1 so that get_uncompleted_crates returns all the crates
        EditExperiment {
            assign: Some(assignee.clone()),
            ..EditExperiment::dummy("dummy")
        }
        .apply(&ctx)
        .unwrap();
        let ex = Experiment::next(&db, &assignee).unwrap().unwrap().1;
        ex.get_uncompleted_crates(&db, &config, &assignee).unwrap();
        METRICS.update_agent_status(&db, &agent_list_ref).unwrap();

        // There are no experiments in the queue but agent1 is still executing the
        // last chunk of the previous experiment
        let status = Metrics::get_metric_by_name(AGENT_WORK_METRIC).unwrap();
        assert!(supposed_to_work(&status, Some(agent1)));
        assert!(!supposed_to_work(&status, Some(agent2)));
    }
}
