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
}
