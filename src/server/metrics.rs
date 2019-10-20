use crate::prelude::*;
use prometheus::{IntCounterVec, __register_counter_vec};

#[derive(Clone)]
pub struct Metrics {
    crater_completed_jobs_total: IntCounterVec,
}

impl Metrics {
    pub fn new() -> Fallible<Self> {
        let opts = prometheus::opts!("crater_completed_jobs_total", "total completed jobs");
        let crater_completed_jobs_total =
            prometheus::register_int_counter_vec!(opts, &["agent", "experiment"])?;
        Ok(Metrics {
            crater_completed_jobs_total,
        })
    }

    pub fn record_completed_jobs(&self, agent: &str, experiment: &str, amount: i64) {
        self.crater_completed_jobs_total
            .with_label_values(&[&agent, &experiment])
            .inc_by(amount);
    }

    fn remove_experiment_jobs(&self, experiment: &str, agents: &[&str]) -> Fallible<()> {
        for agent in agents.iter() {
            self.crater_completed_jobs_total
                .remove_label_values(&[agent, &experiment])?;
        }
        Ok(())
    }

    pub fn on_complete_experiment(&self, experiment: &str, agents: &[&str]) -> Fallible<()> {
        self.remove_experiment_jobs(experiment, agents)
    }
}

#[cfg(test)]
mod tests {
    use super::Metrics;
    use prometheus::proto::MetricFamily;

    const JOBS_METRIC: &str = "crater_completed_jobs_total";

    fn test_experiment_presence(metric: &MetricFamily, experiment: &str) -> bool {
        metric.get_metric().iter().any(|met| {
            met.get_label()
                .iter()
                .any(|lab| lab.get_name() == "experiment" && lab.get_value() == experiment)
        })
    }

    fn get_metric_by_name(name: &str) -> Option<MetricFamily> {
        let families = prometheus::gather();
        families.into_iter().find(|fam| fam.get_name() == name)
    }

    #[test]
    fn test_on_complete_experiment() {
        let ex1 = "pr-0";
        let ex2 = "pr-1";
        let agent1 = "agent-1";
        let agent2 = "agent-2";

        let metrics = Metrics::new().unwrap();
        metrics.record_completed_jobs(agent1, ex1, 1);
        metrics.record_completed_jobs(agent2, ex1, 1);
        metrics.record_completed_jobs(agent2, ex2, 1);

        //test metrics are correctly registered
        let jobs = get_metric_by_name(JOBS_METRIC).unwrap();
        assert!(test_experiment_presence(&jobs, ex1));
        assert!(test_experiment_presence(&jobs, ex2));

        //test metrics are correctly removed when an experiment is completed
        metrics
            .on_complete_experiment(ex1, &[agent2, agent1])
            .unwrap();

        let jobs = get_metric_by_name(JOBS_METRIC).unwrap();
        assert!(!test_experiment_presence(&jobs, ex1));
        assert!(test_experiment_presence(&jobs, ex2));
    }
}
