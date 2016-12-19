#![recursion_limit = "1024"]

#![allow(unused)]
#![feature(proc_macro)]
#![feature(receiver_try_iter)]

extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate tempdir;
extern crate url;
extern crate walkdir;
extern crate json;
extern crate semver;
#[macro_use]
extern crate lazy_static;
extern crate chrono;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde;
extern crate tar;
extern crate flate2;
extern crate toml;
#[macro_use]
extern crate scopeguard;
extern crate libc;

#[macro_use]
mod log;
mod errors;
mod toolchain;
mod compare;
mod registry;
mod lists;
mod file;
mod dl;
mod gh;
mod util;
mod run;
mod crates;
mod git;
mod checkpoint;
mod ex;
mod ex_run;
mod toml_frobber;
mod model;
mod gh_mirrors;
mod report;
mod docker;
mod dirs;

use clap::{App, Arg, AppSettings, SubCommand, ArgMatches};
use errors::*;
use std::panic;
use std::env;
use std::process;
use dirs::*;

fn main() {
    log::init();
    let code = match panic::catch_unwind(main_) {
        Ok(Ok(())) => {
            0
        }
        Ok(Err(e)) => {
            use std::error::Error;
            util::report_error(&e);
            1
        }
        Err(e) => {
            util::report_panic(&*e);
            1
        }
    };
    log::finish();
    process::exit(code);
}

fn main_() -> Result<()> {
    let ref matches = cli().get_matches();

    run_cmd(matches)?;

    Ok(())
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

fn run_cmd(m: &ArgMatches) -> Result<()> {
    let cmd = model::conv::args_to_cmd(m)?;
    let state = model::state::GlobalState::init();
    let _ = model::driver::run(state, cmd)?;

    Ok(())
}

