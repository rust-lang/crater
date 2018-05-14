mod args;
mod commands;

use errors::*;
use futures::future;
use futures::prelude::*;
use hyper::StatusCode;
use hyper::server::{Request, Response};
use ring;
use serde_json;
use server::Data;
use server::github::{EventIssueComment, Issue};
use server::http::{Context, ResponseExt, ResponseFuture};
use server::messages::Message;
use server::routes::webhooks::args::Command;
use std::sync::Arc;
use util;

fn process_webhook(payload: &[u8], signature: &str, event: &str, data: &Data) -> Result<()> {
    if !verify_signature(&data.tokens.bot.webhooks_secret, payload, signature) {
        bail!("invalid signature for the webhook!");
    }

    match event {
        "ping" => info!("the webhook is configured correctly!"),
        "issue_comment" => {
            let p: EventIssueComment = serde_json::from_slice(payload)?;
            if let Err(e) = process_command(&p.sender.login, &p.comment.body, &p.issue, data) {
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

fn process_command(sender: &str, body: &str, issue: &Issue, data: &Data) -> Result<()> {
    let start = format!("@{} ", data.bot_username);
    for line in body.lines() {
        if !line.starts_with(&start) {
            continue;
        }

        let command = line[line.find(' ').unwrap()..].trim();
        if command == "" {
            continue;
        }

        if !data.config.server.bot_acl.contains(sender) {
            Message::new()
                .line(
                    "lock",
                    "**Error:** you're not allowed to interact with this bot.",
                )
                .note(
                    "key",
                    "If you are a member of the Rust team and need access, [add yourself to \
                     the whitelist](\
                     https://github.com/rust-lang-nursery/crater/blob/master/config.toml).",
                )
                .send(&issue.url, data)?;
            return Ok(());
        }

        info!("user @{} sent command: {}", sender, command);

        let args: Command = command.parse().chain_err(|| "failed to parse the command")?;

        match args {
            Command::Ping(_) => {
                commands::ping(data, issue)?;
            }

            Command::Run(args) => {
                commands::run(data, issue, args)?;
            }

            Command::Edit(args) => {
                commands::edit(data, issue, args)?;
            }

            Command::RetryReport(args) => {
                commands::retry_report(data, issue, args)?;
            }

            Command::Abort(args) => {
                commands::abort(data, issue, args)?;
            }
        }

        break;
    }

    Ok(())
}

fn verify_signature(secret: &str, payload: &[u8], raw_signature: &str) -> bool {
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
        .map(|i| *i)
        .collect::<Vec<&str>>()
        .join("=");

    // Convert the signature from hex
    let signature = if let Ok(converted) = util::from_hex(&hex_signature) {
        converted
    } else {
        // This is not hex
        return false;
    };

    // Get the correct digest
    let digest = match *algorithm {
        "sha1" => &ring::digest::SHA1,
        _ => {
            // Unknown digest, return false
            return false;
        }
    };

    // Verify the HMAC signature
    let key = ring::hmac::VerificationKey::new(digest, secret.as_bytes());
    ring::hmac::verify(&key, payload, &signature).is_ok()
}

macro_rules! headers {
    ($req:expr => { $($ident:ident: $name:expr,)* }) => {
        $(
            let option = $req.headers()
                .get_raw($name)
                .and_then(|h| h.one())
                .map(|s| String::from_utf8_lossy(s).to_string());

            let $ident = if let Some(some) = option {
                some
            } else {
                error!("missing header in the webhook: {}", $name);

                return Response::json(&json!({
                    "error": format!("missing header: {}", $name),
                })).unwrap().with_status(StatusCode::BadRequest).as_future();
            };
        )*
    }
}

pub fn handle(req: Request, data: Arc<Data>, ctx: Arc<Context>) -> ResponseFuture {
    headers!(req => {
        signature: "X-Hub-Signature",
        event: "X-GitHub-Event",
    });

    Box::new(req.body().concat2().and_then(move |body| {
        let body = body.iter().cloned().collect::<Vec<u8>>();

        ctx.handle.spawn(ctx.pool.spawn_fn(move || {
            if let Err(err) = process_webhook(&body, &signature, &event, &data) {
                error!("error while processing webhook: {}", err);
            }

            future::ok(())
        }));

        Response::text("OK\n").as_future()
    }))
}
