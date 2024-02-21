mod args;
mod commands;

use crate::prelude::*;
use crate::server::github::{EventIssueComment, Issue, Repository};
use crate::server::messages::Message;
use crate::server::routes::webhooks::args::Command;
use crate::server::{Data, GithubData};
use bytes::Bytes;
use hmac::{Hmac, Mac};
use http::{HeaderMap, Response, StatusCode};
use hyper::Body;
use std::str::FromStr;
use std::sync::Arc;
use warp::{Filter, Rejection};

fn process_webhook(
    payload: &[u8],
    host: &str,
    signature: &str,
    event: &str,
    data: &Data,
    github_data: &GithubData,
) -> Fallible<()> {
    if !verify_signature(&github_data.tokens.webhooks_secret, payload, signature) {
        bail!("invalid signature for the webhook!");
    }

    match event {
        "ping" => info!("the webhook is configured correctly!"),
        "issue_comment" => {
            let p: EventIssueComment = serde_json::from_slice(payload)?;

            // Only process "created" events, and ignore when a comment is edited or deleted
            if p.action != "created" {
                return Ok(());
            }

            crate::server::try_builds::detect(
                &data.db,
                &github_data.api,
                &p.repository.full_name,
                p.issue.number,
                &p.comment.body,
            )?;

            if let Err(e) = process_command(
                host,
                &p.sender.login,
                p.sender.id,
                &p.comment.body,
                &p.repository,
                &p.issue,
                data,
                github_data,
            ) {
                Message::new()
                    .line("rotating_light", format!("**Error:** {e}"))
                    .note(
                        "sos",
                        "If you have any trouble with Crater please ping **`@rust-lang/infra`**!",
                    )
                    .send(&p.issue.url, data, github_data)?;
            }
        }
        e => bail!("invalid event received: {}", e),
    }

    Ok(())
}

fn process_command(
    host: &str,
    sender: &str,
    sender_id: usize,
    body: &str,
    repo: &Repository,
    issue: &Issue,
    data: &Data,
    github_data: &GithubData,
) -> Fallible<()> {
    let start = format!("@{} ", github_data.bot_username);
    for line in body.lines() {
        if !line.starts_with(&start) {
            continue;
        }

        let command = line[line.find(' ').unwrap()..].trim();
        if command.is_empty() {
            continue;
        }

        if !data.acl.allowed(sender, sender_id)? {
            Message::new()
                .line(
                    "lock",
                    "**Error:** you're not allowed to interact with this bot.",
                )
                .note(
                    "key",
                    format!(
                        "If you are a member of the Rust team and need access, [add yourself to \
                         the whitelist]({}/blob/master/config.toml).",
                        crate::CRATER_REPO_URL,
                    ),
                )
                .send(&issue.url, data, github_data)?;
            return Ok(());
        }

        info!("user @{} sent command: {}", sender, command);

        let args: Command =
            Command::from_str(command).with_context(|_| "failed to parse the command")?;

        match args {
            Command::Ping(_) => {
                commands::ping(data, github_data, issue)?;
            }

            Command::Run(args) => {
                commands::run(host, data, github_data, repo, issue, args)?;
            }

            Command::Check(args) => {
                commands::check(host, data, github_data, repo, issue, args)?;
            }

            Command::Edit(args) => {
                commands::edit(data, github_data, issue, args)?;
            }

            Command::RetryReport(args) => {
                commands::retry_report(data, github_data, issue, args)?;
            }

            Command::Retry(args) => {
                commands::retry(data, github_data, issue, args)?;
            }

            Command::Abort(args) => {
                commands::abort(data, github_data, issue, args)?;
            }

            Command::ReloadACL(_) => {
                commands::reload_acl(data, github_data, issue)?;
            }
        }

        break;
    }

    Ok(())
}

fn verify_signature(secret: &str, payload: &[u8], raw_signature: &str) -> bool {
    type HmacSha1 = Hmac<sha1::Sha1>;

    // The signature must have a =
    if !raw_signature.contains('=') {
        return false;
    }

    // Split the raw signature to get the algorithm and the signature
    let splitted: Vec<&str> = raw_signature.split('=').collect();
    let algorithm = &splitted[0];
    let hex_signature = splitted
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<&str>>()
        .join("=");

    // Convert the signature from hex
    let signature = if let Ok(converted) = crate::utils::hex::from_hex(&hex_signature) {
        converted
    } else {
        // This is not hex
        return false;
    };

    // Only SHA-1 is supported
    if *algorithm != "sha1" {
        return false;
    }

    // Verify the HMAC signature
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload);
    mac.verify_slice(&signature).is_ok()
}

fn receive_endpoint(
    data: Arc<Data>,
    github_data: Arc<GithubData>,
    headers: HeaderMap,
    body: Bytes,
) -> Fallible<()> {
    let signature = headers
        .get("X-Hub-Signature")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| err_msg("missing header X-Hub-Signature\n"))?;
    let event = headers
        .get("X-GitHub-Event")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| err_msg("missing header X-GitHub-Event\n"))?;
    let host = headers
        .get("Host")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| err_msg("missing header Host\n"))?;

    process_webhook(&body[..], host, signature, event, &data, &github_data)
}

pub fn routes(
    data: Arc<Data>,
    github_data: Option<Arc<GithubData>>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_filter = warp::any().map(move || data.clone());
    let github_data_filter = warp::any().and_then(move || {
        let g = github_data.clone();
        async move {
            match g {
                Some(github_data) => Ok(github_data),
                None => Err(warp::reject::not_found()),
            }
        }
    });

    warp::post()
        .and(warp::path::end())
        .and(data_filter)
        .and(github_data_filter)
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .map(
            |data: Arc<Data>, github_data: Arc<GithubData>, headers: HeaderMap, body: Bytes| {
                let mut resp: Response<Body>;
                match receive_endpoint(data, github_data, headers, body) {
                    Ok(()) => resp = Response::new("OK\n".into()),
                    Err(err) => {
                        error!("error while processing webhook");
                        crate::utils::report_failure(&err);

                        resp = Response::new(format!("Error: {err}\n").into());
                        *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    }
                }

                resp
            },
        )
}
