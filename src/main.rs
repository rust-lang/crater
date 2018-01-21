#![deny(unused_extern_crates)]
extern crate dotenv;
#[macro_use(slog_info, slog_log, slog_record, slog_record_static, slog_b, slog_kv)]
extern crate slog;
#[macro_use]
extern crate slog_scope;
extern crate structopt;
#[macro_use]
extern crate structopt_derive;

extern crate crater;

mod cli;

use crater::{log, util};
use crater::errors::*;
use std::panic;
use std::process;
use structopt::StructOpt;

fn main() {
    // Ignore errors loading `.env` file.
    let _ = dotenv::dotenv();

    let _guard = log::init();
    let success = match panic::catch_unwind(main_) {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            util::report_error(&e);
            false
        }
        Err(e) => {
            util::report_panic(&*e);
            false
        }
    };
    info!(
        "{}",
        if success {
            "command succeeded"
        } else {
            "command failed"
        }
    );
    log::finish();
    process::exit(if success { 0 } else { 1 });
}

fn main_() -> Result<()> {
    cli::Crater::from_args().run()
}
