use crate::experiments::{Experiment, Status};
use crate::prelude::*;
use crate::report::{self, Comparison, TestResults};
use crate::results::DatabaseDB;
use crate::server::messages::{Label, Message};
use crate::server::Data;
use crate::utils;
use rusoto_core::request::HttpClient;
use rusoto_s3::S3Client;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

// Automatically wake up the reports generator thread every 10 minutes to check for new jobs
const AUTOMATIC_THREAD_WAKEUP: u64 = 600;

fn generate_report(data: &Data, ex: &Experiment, results: &DatabaseDB) -> Fallible<TestResults> {
    let client = S3Client::new_with(
        HttpClient::new()?,
        data.tokens.reports_bucket.to_aws_credentials(),
        data.tokens.reports_bucket.region.to_region()?,
    );
    let dest = format!("s3://{}/{}", data.tokens.reports_bucket.bucket, &ex.name);
    let writer = report::S3Writer::create(Box::new(client), dest.parse()?)?;

    let crates = ex.get_crates(&data.db)?;
    let res = report::gen(results, &ex, &crates, &writer, &data.config, false)?;

    //remove metrics about completed experiments
    data.metrics.on_complete_experiment(&ex.name)?;

    Ok(res)
}

fn reports_thread(data: &Data, wakes: &mpsc::Receiver<()>) -> Fallible<()> {
    let timeout = Duration::from_secs(AUTOMATIC_THREAD_WAKEUP);
    let results = DatabaseDB::new(&data.db);

    loop {
        let mut ex = match Experiment::first_by_status(&data.db, Status::NeedsReport)? {
            Some(ex) => ex,
            None => {
                // This will sleep AUTOMATIC_THREAD_WAKEUP seconds *or* until a wake is received
                if let Err(mpsc::RecvTimeoutError::Disconnected) = wakes.recv_timeout(timeout) {
                    thread::sleep(timeout);
                }

                continue;
            }
        };
        let name = ex.name.clone();

        info!("generating report for experiment {}...", name);
        ex.set_status(&data.db, Status::GeneratingReport)?;

        match generate_report(data, &ex, &results) {
            Err(err) => {
                ex.set_status(&data.db, Status::ReportFailed)?;
                error!("failed to generate the report of {}", name);
                utils::report_failure(&err);

                if let Some(ref github_issue) = ex.github_issue {
                    Message::new()
                        .line(
                            "rotating_light",
                            format!("Report generation of **`{}`** failed: {}", name, err),
                        )
                        .line(
                            "hammer_and_wrench",
                            "If the error is fixed use the `retry-report` command.",
                        )
                        .note(
                            "sos",
                            "Can someone from the infra team check in on this? @rust-lang/infra",
                        )
                        .send(&github_issue.api_url, data)?;
                }

                continue;
            }
            Ok(res) => {
                let base_url = data
                    .tokens
                    .reports_bucket
                    .public_url
                    .replace("{bucket}", &data.tokens.reports_bucket.bucket);
                let report_url = format!("{}/{}/index.html", base_url, name);

                ex.set_status(&data.db, Status::Completed)?;
                ex.set_report_url(&data.db, &report_url)?;
                info!("report for the experiment {} generated successfully!", name);

                let (regressed, fixed) = (
                    res.info.get(&Comparison::Regressed).unwrap_or(&0),
                    res.info.get(&Comparison::Fixed).unwrap_or(&0),
                );

                if let Some(ref github_issue) = ex.github_issue {
                    Message::new()
                        .line("tada", format!("Experiment **`{}`** is completed!", name))
                        .line(
                            "bar_chart",
                            format!(
                                " {} regressed and {} fixed ({} total)",
                                regressed,
                                fixed,
                                res.info.values().sum::<u32>(),
                            ),
                        )
                        .line(
                            "newspaper",
                            format!("[Open the full report]({}).", report_url),
                        )
                        .note(
                            "warning",
                            format!(
                                "If you notice any spurious failure [please add them to the \
                                 blacklist]({}/blob/master/config.toml)!",
                                crate::CRATER_REPO_URL,
                            ),
                        )
                        .set_label(Label::ExperimentCompleted)
                        .send(&github_issue.api_url, data)?;
                }
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct ReportsWorker(Arc<Mutex<Option<mpsc::Sender<()>>>>);

impl ReportsWorker {
    pub fn new() -> Self {
        ReportsWorker(Arc::new(Mutex::new(None)))
    }

    pub fn spawn(&self, data: Data) {
        let waker = self.0.clone();
        thread::spawn(move || {
            // Set up a new waker channel
            let (wake_send, wake_recv) = mpsc::channel();
            {
                let mut waker = waker.lock().unwrap();
                *waker = Some(wake_send);
            }

            loop {
                let result = reports_thread(&data.clone(), &wake_recv)
                    .with_context(|_| "the reports generator thread crashed");
                if let Err(e) = result {
                    utils::report_failure(&e);
                }

                warn!("the reports generator thread will be respawned in one minute");
                thread::sleep(Duration::from_secs(60));
            }
        });
    }

    pub fn wake(&self) {
        // We don't really care if the wake fails: the reports generator thread wakes up on its own
        // every few minutes, so this just speeds up the process
        if let Some(waker) = self.0.lock().ok().as_ref().and_then(|opt| opt.as_ref()) {
            if waker.send(()).is_err() {
                warn!("can't wake the reports generator, will have to wait");
            }
        } else {
            warn!("no report generator to wake up!");
        }
    }
}
