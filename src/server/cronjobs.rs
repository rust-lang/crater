use crate::actions::{Action, ActionsCtx, UpdateLists};
use crate::prelude::*;
use crate::server::Data;
use crate::utils;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DAY: Duration = Duration::from_secs(60 * 60 * 24);

lazy_static! {
    // The tuple is composed of:
    // - job name
    // - interval between executions
    // - function to execute at specified intervals
    static ref JOBS: Vec<(&'static str, Duration, fn(Arc<Data>) -> Fallible<()>)> = {
        let mut jobs = Vec::new();
        jobs.push((
            "crate list update",
            DAY,
            update_crates as fn(Arc<Data>) -> Fallible<()>,
        ));

        jobs
    };
}

pub fn spawn(data: Data) {
    let data = Arc::new(data);
    for job in JOBS.iter() {
        let (name, timer, exec) = job;
        // needed to make the borrowck happy
        let data = Arc::clone(&data);

        thread::spawn(move || loop {
            let result = exec(Arc::clone(&data));
            if let Err(e) = result {
                utils::report_failure(&e);
            }

            info!(
                "the {} thread will be respawned in {}s",
                name,
                timer.as_secs()
            );
            thread::sleep(*timer);
        });
    }
}

fn update_crates(data: Arc<Data>) -> Fallible<()> {
    let ctx = ActionsCtx::new(&data.db, &data.config);

    UpdateLists {
        github: true,
        registry: true,
        local: false,
    }
    .apply(&ctx)
}
