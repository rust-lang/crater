use crate::experiments::{Experiment, Status};
use crate::prelude::*;
use crate::report::{self, Comparison, TestResults, SPURIOUS_RETRY};
use crate::results::DatabaseDB;
use crate::server::messages::{Label, Message};
use crate::server::{Data, GithubData};
use crate::utils;
use std::sync::{Arc, Mutex};
use std::thread::{self, Thread};
use std::time::Duration;

use super::tokens::BucketRegion;

// Automatically wake up the reports generator thread every 10 minutes to check for new jobs
const AUTOMATIC_THREAD_WAKEUP: u64 = 600;

fn generate_report(data: &Data, ex: &Experiment, results: &DatabaseDB) -> Fallible<TestResults> {
    let mut config = aws_config::from_env();
    match &data.tokens.reports_bucket.region {
        BucketRegion::S3 { region } => {
            config = config.region(aws_sdk_s3::config::Region::new(region.to_owned()));
        }
        BucketRegion::Custom { url } => {
            config = config.region(aws_sdk_s3::config::Region::from_static("us-east-1"));
            config = config.endpoint_url(url.clone());
        }
    }
    config = config.credentials_provider(data.tokens.reports_bucket.to_aws_credentials());
    let config = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(config.load());
    let client = aws_sdk_s3::Client::new(&config);
    let writer = report::S3Writer::create(
        client,
        data.tokens.reports_bucket.bucket.clone(),
        ex.name.clone(),
    )?;

    let crates = ex.get_crates(&data.db)?;
    let res = report::gen(results, ex, &crates, &writer, &data.config, false)?;

    //remove metrics about completed experiments
    data.metrics.on_complete_experiment(&ex.name)?;

    Ok(res)
}

fn reports_thread(data: &Data, github_data: Option<&GithubData>) -> Fallible<()> {
    let timeout = Duration::from_secs(AUTOMATIC_THREAD_WAKEUP);
    let results = DatabaseDB::new(&data.db);

    loop {
        let mut ex = match Experiment::ready_for_report(&data.db)? {
            Some(ex) => ex,
            None => {
                // This will sleep AUTOMATIC_THREAD_WAKEUP seconds *or* until a wake is received
                std::thread::park_timeout(timeout);

                continue;
            }
        };
        let name = ex.name.clone();

        info!("generating report for experiment {name}...");
        ex.set_status(&data.db, Status::GeneratingReport)?;

        match generate_report(data, &ex, &results) {
            Err(err) => {
                ex.set_status(&data.db, Status::ReportFailed)?;
                error!("failed to generate the report of {name}");
                utils::report_failure(&err);

                if let Some(github_data) = github_data {
                    if let Some(ref github_issue) = ex.github_issue {
                        Message::new()
                        .line(
                            "rotating_light",
                            format!("Report generation of **`{name}`** failed: {err}"),
                        )
                        .line(
                            "hammer_and_wrench",
                            "If the error is fixed use the `retry-report` command.",
                        )
                        .note(
                            "sos",
                            "If you need assistance dealing with this failure, please ask in [t-infra](https://rust-lang.zulipchat.com/#narrow/channel/242791-t-infra) on Zulip",
                        )
                        .send(&github_issue.api_url, data, github_data)?;
                    }
                }

                continue;
            }
            Ok(res) => {
                let base_url = data
                    .tokens
                    .reports_bucket
                    .public_url
                    .replace("{bucket}", &data.tokens.reports_bucket.bucket);
                let report_url = format!("{base_url}/{name}/index.html");
                let retry_regressed_list_url =
                    format!("{base_url}/{name}/retry-regressed-list.txt");

                ex.set_status(&data.db, Status::Completed)?;
                ex.set_report_url(&data.db, &report_url)?;

                info!("report for the experiment {name} generated successfully!");

                let (regressed, fixed, spurious_retry) = (
                    res.info.get(&Comparison::Regressed).unwrap_or(&0),
                    res.info.get(&Comparison::Fixed).unwrap_or(&0),
                    SPURIOUS_RETRY
                        .iter()
                        .flat_map(|comp| res.info.get(comp))
                        .sum::<u32>(),
                );

                if let Some(github_data) = github_data {
                    if let Some(ref github_issue) = ex.github_issue {
                        Message::new()
                            .line("tada", format!("Experiment **`{name}`** is completed!"))
                            .line(
                                "bar_chart",
                                format!(
                                    "{} regressed and {} fixed ({} total)",
                                    regressed,
                                    fixed,
                                    res.info.values().sum::<u32>(),
                                ),
                            )
                            .line("bar_chart", format!("{spurious_retry} spurious results on the [retry-regressed-list.txt]({retry_regressed_list_url}), consider a retry[^1] if this is a significant amount."))
                            .footnote("1", format!("re-run the experiment with `crates={retry_regressed_list_url}`"))
                            .line(
                                "newspaper",
                                format!("[Open the summary report]({report_url})."),
                            )
                            .note(
                                "warning",
                                format!(
                                    "If you notice any spurious failure [please add them to the \
                                 denylist]({}/blob/HEAD/config.toml)!",
                                    crate::CRATER_REPO_URL,
                                ),
                            )
                            .set_label(Label::ExperimentCompleted)
                            .send(&github_issue.api_url, data, github_data)?;
                    }
                }
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct ReportsWorker(Arc<Mutex<Option<Thread>>>);

impl ReportsWorker {
    pub fn new() -> Self {
        ReportsWorker(Arc::new(Mutex::new(None)))
    }

    pub fn spawn(&self, data: Data, github_data: Option<GithubData>) {
        let joiner = thread::spawn(move || loop {
            let result = reports_thread(&data.clone(), github_data.as_ref())
                .with_context(|| "the reports generator thread crashed");
            if let Err(e) = result {
                utils::report_failure(&e);
            }
        });
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(joiner.thread().clone());
    }

    pub fn wake(&self) {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(thread) = &*guard {
            thread.unpark();
        } else {
            warn!("no report generator to wake up!");
        }
    }
}
