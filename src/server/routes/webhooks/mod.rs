mod args;
mod commands;

use crate::prelude::*;
use crate::server::github::{EventIssueComment, Issue};
use crate::server::messages::Message;
use crate::server::routes::webhooks::args::Command;
use crate::server::Data;
use bytes::buf::Buf;
use http::{HeaderMap, Response, StatusCode};
use hyper::Body;
use openssl::{hash::MessageDigest, pkey::PKey, sign::Signer};
use serde_json;
use std::str::FromStr;
use std::sync::Arc;
use warp::{self, filters::body::FullBody, Filter, Rejection};

fn process_webhook(
    payload: &[u8],
    host: &str,
    signature: &str,
    event: &str,
    data: &Data,
) -> Fallible<()> {
    if !verify_signature(&data.tokens.bot.webhooks_secret, payload, signature) {
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

            if let Err(e) = process_command(host, &p.sender.login, &p.comment.body, &p.issue, data)
            {
                Message::new()
                    .line("rotating_light", format!("**Error:** {}", e))
                    .note(
                        "sos",
                        "If you have any trouble with Crater please ping **`@rust-lang/infra`**!",
                    )
                    .send(&p.issue.url, data)?;
            }
        }
        e => bail!("invalid event received: {}", e),
    }

    Ok(())
}

fn process_command(
    host: &str,
    sender: &str,
    body: &str,
    issue: &Issue,
    data: &Data,
) -> Fallible<()> {
    let start = format!("@{} ", data.bot_username);
    for line in body.lines() {
        if !line.starts_with(&start) {
            continue;
        }

        let command = line[line.find(' ').unwrap()..].trim();
        if command == "" {
            continue;
        }

        if !data.acl.allowed(sender) {
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
                .send(&issue.url, data)?;
            return Ok(());
        }

        info!("user @{} sent command: {}", sender, command);

        let args: Command =
            Command::from_str(command).with_context(|_| "failed to parse the command")?;

        match args {
            Command::Ping(_) => {
                commands::ping(data, issue)?;
            }

            Command::Run(args) => {
                commands::run(host, data, issue, args)?;
            }

            Command::Edit(args) => {
                commands::edit(data, issue, args)?;
            }

            Command::RetryReport(args) => {
                commands::retry_report(data, issue, args)?;
            }

            Command::Retry(args) => {
                commands::retry(data, issue, args)?;
            }

            Command::Abort(args) => {
                commands::abort(data, issue, args)?;
            }

            Command::ReloadACL(_) => {
                commands::reload_acl(data, issue)?;
            }
        }

        break;
    }

    Ok(())
}

fn verify_signature(secret: &str, payload: &[u8], raw_signature: &str) -> bool {
    macro_rules! try_false {
        ($expr:expr) => {
            match $expr {
                Ok(res) => res,
                Err(_) => return false,
            }
        };
    };

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

    // Get the correct digest
    let digest = match *algorithm {
        "sha1" => MessageDigest::sha1(),
        // Unknown digest, return false
        _ => return false,
    };

    // Verify the HMAC using OpenSSL
    let key = try_false!(PKey::hmac(secret.as_bytes()));
    let mut signer = try_false!(Signer::new(digest, &key));
    try_false!(signer.update(payload));
    let hmac = try_false!(signer.sign_to_vec());
    openssl::memcmp::eq(&hmac, &signature)
}

fn receive_endpoint(data: Arc<Data>, headers: HeaderMap, body: FullBody) -> Fallible<()> {
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

    process_webhook(body.bytes(), host, signature, event, &data)
}

pub fn routes(
    data: Arc<Data>,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Clone {
    let data_filter = warp::any().map(move || data.clone());

    warp::post2()
        .and(warp::path::end())
        .and(data_filter)
        .and(warp::header::headers_cloned())
        .and(warp::body::concat())
        .map(|data: Arc<Data>, headers: HeaderMap, body: FullBody| {
            let mut resp: Response<Body>;
            match receive_endpoint(data, headers, body) {
                Ok(()) => resp = Response::new("OK\n".into()),
                Err(err) => {
                    error!("error while processing webhook");
                    crate::utils::report_failure(&err);

                    resp = Response::new(format!("Error: {}\n", err).into());
                    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                }
            }

            resp
        })
}
