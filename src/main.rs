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
mod toml_frobber;

use clap::{App, Arg, AppSettings, SubCommand, ArgMatches};
use errors::*;
use std::panic;
use std::env;
use std::process;

const WORK_DIR: &'static str = "./work";
const CARGO_HOME: &'static str = "./work/cargo-home";
const RUSTUP_HOME: &'static str = "./work/rustup-home";
const TOOLCHAIN_DIR: &'static str = "./work/tc";
const CRATES_DIR: &'static str = "./work/crates";
const EXPERIMENT_DIR: &'static str = "./work/ex";
const LIST_DIR: &'static str = "./work/lists";
const LOG_DIR: &'static str = "./work/logs";
const FROB_DIR: &'static str = "./work/fromls";
const TARGET_DIR: &'static str = "./work/target-dirs";
const TEST_DIR: &'static str = "./work/test";

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

    match matches.subcommand() {
        ("create-recent-list", Some(_)) => create_recent_list()?,
        ("create-second-list", Some(_)) => create_second_list()?,
        ("create-hot-list", Some(_)) => create_hot_list()?,
        ("create-gh-candidate-list", Some(_)) => create_gh_candidate_list()?,
        ("create-gh-app-list", Some(_)) => create_gh_app_list()?,
        ("prepare-crates", Some(_)) => prepare_crates()?,
        ("prepare-toolchain", Some(m)) => prepare_toolchain(m)?,
        ("link-toolchain", Some(m)) => panic!(),
        ("frob-cargo-tomls", Some(_)) => frob_cargo_tomls()?,
        ("capture-shas", Some(m)) => capture_shas(m)?,
        ("capture-lockfiles", Some(m)) => capture_lockfiles(m)?,
        ("fetch-deps", Some(m)) => fetch_deps(m)?,
        ("run", Some(m)) => run(m)?,
        ("summarize", Some(_)) => panic!(),
        ("easy-test", Some(m)) => panic!(),
        ("sleep", Some(m)) => sleep(m)?,
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

        .subcommand(
            SubCommand::with_name("create-recent-list")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("create-second-list")
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
        .subcommand(
            SubCommand::with_name("prepare-crates")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("prepare-toolchain")
                .about("TODO")
                .arg(Arg::with_name("toolchain").required(true))
                .arg(Arg::with_name("target").required(true)))
        .subcommand(
            SubCommand::with_name("frob-cargo-tomls")
                .about("TODO"))
        .subcommand(
            SubCommand::with_name("capture-shas")
                .about("TODO")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("capture-lockfiles")
                .about("TODO")
                .arg(Arg::with_name("toolchain")
                     .long("toolchain")
                     .required(true)
                     .takes_value(true))
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default"))
                .arg(Arg::with_name("all")
                     .long("all")))
        .subcommand(
            SubCommand::with_name("fetch-deps")
                .about("TODO")
                .arg(Arg::with_name("toolchain")
                     .long("toolchain")
                     .required(true)
                     .takes_value(true))
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("run")
                .arg(Arg::with_name("toolchain")
                     .long("toolchain")
                     .required(true)
                     .takes_value(true))
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("summarize")
                .about("TODO"))
                .arg(Arg::with_name("name")
                     .long("name")
                     .required(false)
                     .default_value("default"))
        .subcommand(
            SubCommand::with_name("sleep")
                .arg(Arg::with_name("secs")
                     .required(true)))


}

fn prepare_toolchain(m: &ArgMatches) -> Result<()> {
    let ref toolchain = m.value_of("toolchain").expect("");
    let ref target = m.value_of("target").expect("");
    toolchain::prepare_toolchain(toolchain, target)
}

fn create_recent_list() -> Result<()> {
    lists::create_recent_list()
}

fn create_second_list() -> Result<()> {
    lists::create_second_list()
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

fn prepare_crates() -> Result<()> {
    crates::prepare()
}

fn frob_cargo_tomls() -> Result<()> {
    toml_frobber::frob_em()
}

fn capture_shas(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::capture_shas(ex_name)
}

fn capture_lockfiles(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    let ref toolchain = m.value_of("toolchain").expect("");
    let all = m.value_of("all").is_some();
    ex::capture_lockfiles(ex_name, toolchain, all)
}

fn fetch_deps(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    let ref toolchain = m.value_of("toolchain").expect("");
    ex::fetch_deps(ex_name, toolchain)
}

fn run(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    let ref toolchain = m.value_of("toolchain").expect("");
    ex::run_build_and_test_test(ex_name, toolchain)
}

fn sleep(m: &ArgMatches) -> Result<()> {
    let ref secs = m.value_of("secs").expect("");
    run::run("sleep", &[secs], &[]);
    Ok(())
}
