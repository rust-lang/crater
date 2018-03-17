use errors::*;
use futures::future;
use futures::prelude::*;
use hyper::header::ContentLength;
use hyper::server::{Request, Response};
use ring;
use serde_json;
use server::Data;
use server::github::EventIssueComment;
use server::http::{Context, ResponseFuture};
use std::sync::Arc;
use util;

fn process_webhook(payload: &str, signature: &str, event: &str, data: &Data) -> Result<()> {
    if !verify_signature(&data.tokens.bot.webhooks_secret, payload, signature) {
        bail!("invalid signature for the webhook!");
    }

    match event {
        "ping" => info!("the webhook is configured correctly!"),
        "issue_comment" => {
            let p: EventIssueComment = serde_json::from_str(payload)?;
            process_command(&p.sender.login, &p.comment.body, &p.comment.issue_url, data)?;
        }
        e => bail!("invalid event received: {}", e),
    }

    Ok(())
}

fn process_command(sender: &str, body: &str, issue_url: &str, data: &Data) -> Result<()> {
    let start = format!("@{} ", data.bot_username);
    for line in body.lines() {
        if line.starts_with(&start) {
            let command = line.split(' ').skip(1).collect::<Vec<_>>();
            if command.is_empty() {
                continue;
            }

            info!("user @{} sent command: {}", sender, command.join(" "));

            if command.len() == 1 && command[0] == "ping" {
                data.github.post_comment(issue_url, ":tennis: *Pong!*")?;
            }

            break;
        }
    }

    Ok(())
}

fn verify_signature(secret: &str, payload: &str, raw_signature: &str) -> bool {
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
    ring::hmac::verify(&key, payload.as_bytes(), &signature).is_ok()
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

                let msg = format!("Error: missing header: {}\n", $name);
                return Box::new(future::ok(
                    Response::new()
                        .with_header(ContentLength(msg.len() as u64))
                        .with_body(msg)
                        .with_status(::hyper::StatusCode::BadRequest)
                ));
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
        let body = String::from_utf8_lossy(&body.iter().cloned().collect::<Vec<u8>>()).to_string();

        ctx.handle.spawn(ctx.pool.spawn_fn(move || {
            if let Err(err) = process_webhook(&body, &signature, &event, &data) {
                error!("error while processing webhook: {}", err);
            }

            future::ok(())
        }));

        let message = "OK\n";
        future::ok(
            Response::new()
                .with_header(ContentLength(message.len() as u64))
                .with_body(message),
        )
    }))
}
