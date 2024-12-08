use crate::agent::Capabilities;
use crate::experiments::{Assignee, Experiment};
use crate::prelude::*;
use crate::results::{DatabaseDB, EncodingType, ProgressData};
use crate::server::agents::WorkerInfo;
use crate::server::api_types::{AgentConfig, ApiResponse};
use crate::server::auth::{auth_filter, AuthDetails};
use crate::server::messages::Message;
use crate::server::{Data, GithubData, HttpError};
use crossbeam_channel::Sender;
use failure::Compat;
use http::Response;
use hyper::Body;
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;
use warp::{Filter, Rejection};

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ExperimentData<T> {
    experiment_name: String,
    #[serde(flatten)]
    data: T,
}

pub fn routes(
    data: Arc<Data>,
    mutex: Arc<Mutex<Data>>,
    github_data: Option<Arc<GithubData>>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_cloned = data.clone();
    let data_filter = warp::any().map(move || data_cloned.clone());
    let mutex_filter = warp::any().map(move || mutex.clone());
    let github_data_filter = warp::any().map(move || github_data.clone());

    let config = warp::post()
        .and(warp::path("config"))
        .and(warp::path::end())
        .and(warp::body::json())
        .and(data_filter.clone())
        .and(auth_filter(data.clone()))
        .map(endpoint_config);

    let next_experiment = warp::post()
        .and(warp::path("next-experiment"))
        .and(warp::path::end())
        .and(mutex_filter.clone())
        .and(github_data_filter)
        .and(auth_filter(data.clone()))
        .map(endpoint_next_experiment);

    let next_crate = warp::post()
        .and(warp::path("next-crate"))
        .and(warp::path::end())
        .and(warp::body::json())
        .and(data_filter.clone())
        .and(auth_filter(data.clone()))
        .map(endpoint_next_crate);

    let record_progress = warp::post()
        .and(warp::path("record-progress"))
        .and(warp::path::end())
        .and(warp::body::json())
        .and(data_filter.clone())
        .and(auth_filter(data.clone()))
        .map(endpoint_record_progress);

    let heartbeat = warp::post()
        .and(warp::path("heartbeat"))
        .and(warp::path::end())
        .and(warp::body::json())
        .and(data_filter)
        .and(auth_filter(data.clone()))
        .map(endpoint_heartbeat);

    let error = warp::post()
        .and(warp::path("error"))
        .and(warp::path::end())
        .and(warp::body::json())
        .and(mutex_filter)
        .and(auth_filter(data))
        .map(endpoint_error);

    warp::any()
        .and(
            config
                .or(next_experiment)
                .unify()
                .or(next_crate)
                .unify()
                .or(record_progress)
                .unify()
                .or(heartbeat)
                .unify()
                .or(error)
                .unify(),
        )
        .map(handle_results)
        .recover(handle_errors)
        .unify()
}

fn endpoint_config(
    caps: Capabilities,
    data: Arc<Data>,
    auth: AuthDetails,
) -> Fallible<Response<Body>> {
    data.agents.add_capabilities(&auth.name, &caps)?;

    Ok(ApiResponse::Success {
        result: AgentConfig {
            agent_name: auth.name,
            crater_config: data.config.clone(),
        },
    }
    .into_response()?)
}

fn endpoint_next_experiment(
    mutex: Arc<Mutex<Data>>,
    github_data: Option<Arc<GithubData>>,
    auth: AuthDetails,
) -> Fallible<Response<Body>> {
    //we need to make sure that Experiment::next executes uninterrupted
    let data = mutex.lock().unwrap();
    let next = Experiment::next(&data.db, &Assignee::Agent(auth.name))?;
    let result = if let Some((new, ex)) = next {
        if new {
            if let Some(github_data) = github_data.as_ref() {
                if let Some(ref github_issue) = ex.github_issue {
                    Message::new()
                        .line(
                            "construction",
                            format!("Experiment **`{}`** is now **running**", ex.name,),
                        )
                        .send(&github_issue.api_url, &data, github_data)?;
                }
            }
        }

        Some(ex)
    } else {
        None
    };

    Ok(ApiResponse::Success { result }.into_response()?)
}

fn endpoint_next_crate_inner(
    experiment: String,
    data: Arc<Data>,
) -> Fallible<Option<crate::crates::Crate>> {
    let result: Option<crate::crates::Crate> =
        if let Some(ex) = Experiment::get(&data.db, &experiment)? {
            while let Some(next) = data.uncompleted_cache.lock().unwrap().pop_front() {
                if next.0.elapsed() <= std::time::Duration::from_secs(60) {
                    return Ok(Some(next.1));
                }
            }

            let mut crates = ex.get_uncompleted_crates(&data.db, Some(100))?;
            if crates.is_empty() {
                None
            } else {
                let now = std::time::Instant::now();
                let ret = crates.pop().unwrap();
                data.uncompleted_cache
                    .lock()
                    .unwrap()
                    .extend(crates.into_iter().map(|c| (now, c)));
                Some(ret)
            }
        } else {
            None
        };

    Ok(result)
}

fn endpoint_next_crate(
    experiment: String,
    data: Arc<Data>,
    _auth: AuthDetails,
) -> Fallible<Response<Body>> {
    Ok(ApiResponse::Success {
        result: endpoint_next_crate_inner(experiment, data)?,
    }
    .into_response()?)
}

#[derive(Clone)]
pub struct RecordProgressThread {
    // String is the worker name
    queue: Sender<ExperimentData<ProgressData>>,
    in_flight_requests: Arc<(Mutex<usize>, Condvar)>,
}

impl RecordProgressThread {
    pub fn new(
        db: crate::db::Database,
        metrics: crate::server::metrics::Metrics,
    ) -> RecordProgressThread {
        // 64 message queue, after which we start load shedding automatically.
        let (tx, rx) = crossbeam_channel::bounded(64);
        let in_flight_requests = Arc::new((Mutex::new(0), Condvar::new()));

        let this = RecordProgressThread {
            queue: tx,
            in_flight_requests,
        };
        let ret = this.clone();
        std::thread::spawn(move || loop {
            // Panics should already be logged and otherwise there's not much we
            // can/should do.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let result = rx.recv().unwrap();
                this.block_until_idle();

                let start = std::time::Instant::now();

                if let Some(ex) = Experiment::get(&db, &result.experiment_name).unwrap() {
                    let db = DatabaseDB::new(&db);
                    if let Err(e) = db.store(&ex, &result.data, EncodingType::Plain) {
                        // Failing to record a result is basically fine -- this
                        // just means that we'll have to re-try this job.
                        log::error!("Failed to store result into database: {:?}", e);
                        crate::utils::report_failure(&e);
                    }

                    metrics.record_completed_jobs(&ex.name, 1);

                    if let Err(e) = db.clear_stale_records() {
                        // Not a hard failure. We can continue even if we failed
                        // to clear records from already completed runs...
                        log::error!("Failed to clear stale records: {:?}", e);
                        crate::utils::report_failure(&e);
                    }

                    metrics
                        .crater_endpoint_time
                        .with_label_values(&["record_progress_worker"])
                        .observe(start.elapsed().as_secs_f64());

                    metrics
                        .crater_progress_report
                        .with_label_values(&[
                            ex.name.as_str(),
                            &result.data.result.result.to_string(),
                        ])
                        .inc();
                }
            }));
        });

        ret
    }

    pub fn block_until_idle(&self) {
        // Wait until there are zero in-flight requests.
        //
        // Note: We do **not** keep the lock here for the subsequent
        // computation. That means that if we ever observe zero, then we're
        // going to kick off the below computation; obviously requests may keep
        // coming in -- we don't want to block those requests.
        //
        // The expectation that we will see zero here also implies that
        // the server is *sometimes* idle (i.e., we are not constantly
        // processing requests at 100% load). It's not clear that's 100%
        // a valid assumption, but if we are at 100% load in terms of
        // requests coming in, that's a problem in and of itself (since
        // the majority of expected requests are record-progress, which
        // should be *very* fast now that the work for them is async and
        // offloaded to this thread).
        //
        // Ignore the mutex guard (see above).
        drop(
            self.in_flight_requests
                .1
                .wait_while(
                    self.in_flight_requests
                        .0
                        .lock()
                        .unwrap_or_else(|l| l.into_inner()),
                    |g| *g != 0,
                )
                .unwrap_or_else(|g| g.into_inner()),
        );
    }

    pub fn start_request(&self) -> RequestGuard {
        *self
            .in_flight_requests
            .0
            .lock()
            .unwrap_or_else(|l| l.into_inner()) += 1;
        RequestGuard {
            thread: self.clone(),
        }
    }
}

pub struct RequestGuard {
    thread: RecordProgressThread,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        *self
            .thread
            .in_flight_requests
            .0
            .lock()
            .unwrap_or_else(|l| l.into_inner()) -= 1;
        self.thread.in_flight_requests.1.notify_one();
    }
}

// This endpoint does not use the mutex data wrapper to exclude running in
// parallel with other endpoints, which may mean that we (for example) are
// recording results for an abort'd experiment. This should generally be fine --
// the database already has foreign_keys enabled and that should ensure
// appropriate synchronization elsewhere. (If it doesn't, that's mostly a bug
// elsewhere, not here).
//
// In practice it's pretty likely that we won't fully run in parallel anyway,
// but this lets some of the work proceed without the lock being held, which is
// generally positive.
fn endpoint_record_progress(
    result: ExperimentData<ProgressData>,
    data: Arc<Data>,
    _auth: AuthDetails,
) -> Fallible<Response<Body>> {
    let start = Instant::now();

    data.metrics
        .result_log_size
        .observe(result.data.result.log.len() as f64);

    let ret = match data.record_progress_worker.queue.try_send(result) {
        Ok(()) => Ok(ApiResponse::Success { result: true }.into_response()?),
        Err(crossbeam_channel::TrySendError::Full(_)) => {
            data.metrics.crater_bounced_record_progress.inc_by(1);
            Ok(ApiResponse::<()>::SlowDown.into_response()?)
        }
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => unreachable!(),
    };

    data.metrics
        .crater_endpoint_time
        .with_label_values(&["record_progress_endpoint"])
        .observe(start.elapsed().as_secs_f64());

    ret
}

fn endpoint_heartbeat(
    id: WorkerInfo,
    data: Arc<Data>,
    auth: AuthDetails,
) -> Fallible<Response<Body>> {
    data.agents.add_worker(id);
    if let Some(rev) = auth.git_revision {
        data.agents.set_git_revision(&auth.name, &rev)?;
    }

    data.agents.record_heartbeat(&auth.name)?;
    data.metrics
        .record_worker_count(data.agents.active_worker_count());
    Ok(ApiResponse::Success { result: true }.into_response()?)
}

fn endpoint_error(
    error: ExperimentData<HashMap<String, String>>,
    mutex: Arc<Mutex<Data>>,
    auth: AuthDetails,
) -> Fallible<Response<Body>> {
    log::error!(
        "agent {} failed while running {}: {:?}",
        auth.name,
        error.experiment_name,
        error.data.get("error")
    );

    let data = mutex.lock().unwrap();
    let ex = Experiment::get(&data.db, &error.experiment_name)?
        .ok_or_else(|| err_msg("no experiment run by this agent"))?;

    data.metrics.record_error(&auth.name, &ex.name);

    Ok(ApiResponse::Success { result: true }.into_response()?)
}

fn handle_results(resp: Fallible<Response<Body>>) -> Response<Body> {
    match resp {
        Ok(resp) => resp,
        Err(err) => ApiResponse::internal_error(err.to_string())
            .into_response()
            .unwrap(),
    }
}

async fn handle_errors(err: Rejection) -> Result<Response<Body>, Rejection> {
    let error = if let Some(compat) = err.find::<Compat<HttpError>>() {
        Some(*compat.get_ref())
    } else if err.is_not_found() {
        Some(HttpError::NotFound)
    } else {
        None
    };

    match error {
        Some(HttpError::NotFound) => Ok(ApiResponse::not_found().into_response().unwrap()),
        Some(HttpError::Forbidden) => Ok(ApiResponse::unauthorized().into_response().unwrap()),
        None => Err(err),
    }
}
