use crate::actions::{Action, ActionsCtx, UpdateLists};
use crate::prelude::*;
use crate::server::Data;
use crate::utils;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DAY: Duration = Duration::from_secs(60 * 60 * 24);
struct JobDescription {
    name: &'static str,
    interval: Duration,
    exec: fn(Arc<Data>) -> Fallible<()>,
}

static JOBS: &[JobDescription] = &[JobDescription {
    name: "crates lists update",
    interval: DAY,
    exec: update_crates as fn(Arc<Data>) -> Fallible<()>,
}];

pub fn spawn(data: Data) {
    let data = Arc::new(data);
    for job in JOBS {
        // needed to make the borrowck happy
        let data = Arc::clone(&data);

        thread::spawn(move || loop {
            let result = (job.exec)(Arc::clone(&data));
            if let Err(e) = result {
                utils::report_failure(&e);
            }

            info!(
                "the {} thread will be respawned in {}s",
                job.name,
                job.interval.as_secs()
            );
            thread::sleep(job.interval);
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
