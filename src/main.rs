#![deny(unused_extern_crates)]
extern crate clap;
extern crate dotenv;
extern crate result;
#[macro_use(slog_info, slog_log, slog_record, slog_record_static, slog_b, slog_kv)]
extern crate slog;
#[macro_use]
extern crate slog_scope;

extern crate cargobomb;

mod model;

use cargobomb::{log, util};
use cargobomb::errors::*;
use clap::{App, AppSettings};
use std::panic;
use std::process;

fn main() {
    // Ignore errors loading `.env` file.
    let _ = dotenv::dotenv();

    let _guard = log::init();
    let success = match panic::catch_unwind(main_) {
        Ok(Ok(())) => {
            true
        }
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
    let matches = cli().get_matches();
    let cmd = model::conv::clap_args_to_cmd(&matches)?;
    cmd.run()
}

fn cli() -> App<'static, 'static> {
    App::new("cargobomb")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Kaboom!")
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::DeriveDisplayOrder)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommands(model::conv::clap_cmds())
}
