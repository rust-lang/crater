#![recursion_limit = "1024"]

#![allow(unused)]
#![feature(question_mark)]

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

use clap::{App, Arg, AppSettings, SubCommand, ArgMatches};
use errors::*;
use std::panic;
use std::env;

const WORK_DIR: &'static str = "./work";
const CARGO_HOME: &'static str = "./work/cargo-home";
const RUSTUP_HOME: &'static str = "./work/rustup-home";
const TOOLCHAIN_DIR: &'static str = "./work/tc";
const REPO_DIR: &'static str = "./work/repos";
const EXPERIMENT_DIR: &'static str = "./work/ex";
const DISCO_DIR: &'static str = "./work/disco";
const LIST_DIR: &'static str = "./work/lists";
const LOG_DIR: &'static str = "./work/logs";

fn main() {
    log!("program args: {}", env::args().skip(1).collect::<Vec<_>>().join(" "));
    let r = panic::catch_unwind(|| {
        if let Err(e) = main_() {
            log!("error: {}", e);
            std::process::exit(1);
        }
    });
    if let Err(e) = r {
        log!("panic: {:?}", e);
    }
    log::finish();
}

fn main_() -> Result<()> {
    let ref matches = cli().get_matches();

    match matches.subcommand() {
        ("prepare-toolchain", Some(m)) => try!(prepare_toolchain(m)),
        ("run", Some(m)) => try!(run(m)),
        ("create-recent-list", Some(_)) => try!(create_recent_list()),
        ("create-hot-list", Some(_)) => try!(create_hot_list()),
        ("create-gh-candidate-list", Some(_)) => try!(create_gh_candidate_list()),
        ("create-gh-app-list", Some(_)) => try!(create_gh_app_list()),
        _ => unreachable!()
    }

    Ok(())
}

fn cli() -> App<'static, 'static> {
    App::new("cargobomb")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Kaboom!")
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::DeriveDisplayOrder)
        .setting(AppSettings::SubcommandRequiredElseHelp)

        // Main commands
        .subcommand(
            SubCommand::with_name("prepare-lists")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("prepare-toolchain")
                .about("TODO")
                .arg(Arg::with_name("toolchain").required(true))
                .arg(Arg::with_name("target").required(true)))
        .subcommand(
            SubCommand::with_name("prepare-crates")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("test")
                .about("Run a single experiment")
                .arg(Arg::with_name("toolchain1").required(true))
                .arg(Arg::with_name("toolchain2").required(true))
                .arg(Arg::with_name("target").required(true))
                .arg(Arg::with_name("name")
                     .long("name")
                     .required(false)
                     .default_value("default"))
                .arg(Arg::with_name("mode")
                     .long("mode")
                     .required(false)
                     .possible_values(&["release", "debug"])
                     .default_value("debug"))
                .arg(Arg::with_name("rustflags")
                     .long("rustflags")
                     .required(false))
                .arg(Arg::with_name("crate-list-file")
                     .long("crate-list-file")
                     .required(false)
                     .default_value("../crate-list.txt")))
        .subcommand(
            SubCommand::with_name("summarize")
                .about("TODO"))
                .arg(Arg::with_name("name")
                     .long("name")
                     .required(false)
                     .default_value("default"))

        // Additional commands
        .subcommand(
            SubCommand::with_name("create-recent-list")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("create-hot-list")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("create-gh-candidate-list")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("create-gh-app-list")
                .about("TODO"))
}

fn prepare_toolchain(m: &ArgMatches) -> Result<()> {
    let ref toolchain = m.value_of("toolchain").expect("");
    let ref target = m.value_of("target").expect("");
    toolchain::prepare_toolchain(toolchain, target)
}

fn run(m: &ArgMatches) -> Result<()> {
    use compare::*;

    let toolchain1 = m.value_of("toolchain1").expect("");
    let toolchain2 = m.value_of("toolchain2").expect("");
    let target = m.value_of("target").expect("");
    let mode = m.value_of("mode").expect("");
    let rustflags = m.value_of("rustflags");
    let crate_list_file = m.value_of("crate-list-file").expect("");

    let mode = if mode == "debug" { Mode::Debug } else { Mode::Release };
    let crates = load_crate_list(&crate_list_file)?;
    let config = Config {
        toolchain1: toolchain1.to_string(),
        toolchain2: toolchain2.to_string(),
        target: target.to_string(),
        mode: mode,
        rustflags: rustflags.map(|m| m.to_string()),
        crates: crates,
    };

    compare::run(&config)
}

fn create_recent_list() -> Result<()> {
    lists::create_recent_list()
}

fn create_hot_list() -> Result<()> {
    lists::create_hot_list()
}

fn create_gh_candidate_list() -> Result<()> {
    lists::create_gh_candidate_list()
}

fn create_gh_app_list() -> Result<()> {
    lists::create_gh_app_list()
}
