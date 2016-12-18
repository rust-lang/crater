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

use clap::{App, Arg, AppSettings, SubCommand, ArgMatches};
use errors::*;
use std::panic;
use std::env;
use std::process;

const WORK_DIR: &'static str = "./work";
const LOCAL_DIR: &'static str = "./work/local";

const CARGO_HOME: &'static str = "./work/local/cargo-home";
const RUSTUP_HOME: &'static str = "./work/local/rustup-home";

// Custom toolchains
const TOOLCHAIN_DIR: &'static str = "./work/local/custom-tc";

// Where cargo puts its output, when running outside a docker container,
// CARGO_TARGET_DIR
const TARGET_DIR: &'static str = "./work/local/target-dirs";

// The directory crates are unpacked to for running tests, mounted
// in docker containers
const TEST_SOURCE_DIR: &'static str = "./work/local/test-source";

// Where GitHub crate mirrors are stored
const GH_MIRRORS_DIR: &'static str = "./work/local/gh-mirrors";

// Where crates.io sources are stores
const CRATES_DIR: &'static str = "./work/shared/crates";

// Lists of crates
const LIST_DIR: &'static str = "./work/shared/lists";

// crates.io Cargo.toml files, modified to build correctly
const FROB_DIR: &'static str = "./work/shared/fromls";

const EXPERIMENT_DIR: &'static str = "./work/ex";
const LOG_DIR: &'static str = "./work/logs";

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
        // Local prep
        ("prepare-local", Some(_)) => prepare_local()?,
        ("prepare-toolchain", Some(m)) => prepare_toolchain(m)?,
        ("build-container", Some(_)) => build_container()?,

        // List creation
        ("create-lists", Some(m)) => create_lists(m)?,
        ("create-recent-list", Some(_)) => create_recent_list()?,
        ("create-second-list", Some(_)) => create_second_list()?,
        ("create-hot-list", Some(_)) => create_hot_list()?,
        ("create-gh-candidate-list", Some(_)) => create_gh_candidate_list()?,
        ("create-gh-app-list", Some(_)) => create_gh_app_list()?,
        ("create-gh-candidate-list-from-cache", Some(_)) => create_gh_candidate_list_from_cache()?,
        ("create-gh-app-list-from-cache", Some(_)) => create_gh_app_list_from_cache()?,

        // Experiment prep
        ("define-ex", Some(m)) => define_ex(m)?,
        ("prepare-ex", Some(m)) => prepare_ex(m)?,
        ("copy-ex", Some(m)) => copy_ex(m)?,
        ("delete-ex", Some(m)) => delete_ex(m)?,

        // Global experiment prep
        ("prepare-ex-shared", Some(m)) => prepare_ex_shared(m)?,
        ("fetch-gh-mirrors", Some(m)) => fetch_gh_mirrors(m)?,
        ("capture-shas", Some(m)) => capture_shas(m)?,
        ("download-crates", Some(m)) => download_crates(m)?,
        ("frob-cargo-tomls", Some(m)) => frob_cargo_tomls(m)?,
        ("capture-lockfiles", Some(m)) => capture_lockfiles(m)?,

        // Local experiment prep
        ("prepare-ex-local", Some(m)) => prepare_ex_local(m)?,
        ("fetch-deps", Some(m)) => fetch_deps(m)?,
        ("prepare-all-toolchains-for-ex", Some(m)) => prepare_all_toolchains_for_ex(m)?,
        ("delete-all-target-dirs-for-ex", Some(m)) => delete_all_target_dirs_for_ex(m)?,

        // Experimenting
        ("run", Some(m)) => run(m)?,
        ("run-tc", Some(m)) => run_tc(m)?,
        ("delete-all-results", Some(m)) => delete_all_results(m)?,

        // Reporting
        ("gen-report", Some(m)) => gen_report(m)?,

        // Misc
        ("link-toolchain", Some(m)) => panic!(),
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


        // Local prep
        .subcommand(
           SubCommand::with_name("prepare-local")
                .about("acquire toolchains, build containers, build crate lists"))
        .subcommand(
            SubCommand::with_name("prepare-toolchain")
                .about("install or update a toolchain")
                .arg(Arg::with_name("toolchain").required(true)))
        .subcommand(
           SubCommand::with_name("build-container")
                .about("build docker container needed by experiments"))

        // Lists
        .subcommand(
            SubCommand::with_name("create-lists")
                .about("create all the lists of crates")
                .arg(Arg::with_name("full")
                     .long("full")
                     .required(false)
                     .takes_value(false)))
        .subcommand(
            SubCommand::with_name("create-recent-list")
                .about("create the list of most recent crate versions"))
        .subcommand(
            SubCommand::with_name("create-second-list")
                .about("create the list of of second-most-recent crate versions"))
        .subcommand(
            SubCommand::with_name("create-hot-list")
                .about("create the list of popular crates"))
        .subcommand(
            SubCommand::with_name("create-gh-candidate-list")
                .about("crate the list of all GitHub Rust repos"))
        .subcommand(
            SubCommand::with_name("create-gh-app-list")
                .about("create the list of GitHub Rust applications"))
        .subcommand(
            SubCommand::with_name("create-gh-candidate-list-from-cache")
                .about("crate the list of all GitHub Rust repos from cache"))
        .subcommand(
            SubCommand::with_name("create-gh-app-list-from-cache")
                .about("create the list of GitHub Rust applications from cache"))

        // Experiment prep
        .subcommand(
            SubCommand::with_name("define-ex")
                .about("define an experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default"))
                .arg(Arg::with_name("toolchain")
                     .long("toolchain")
                     .required(true)
                     .multiple(true)
                     .takes_value(true))
                .arg(Arg::with_name("type")
                     .long("type")
                     .required(false)
                     .default_value("build-and-test")
                     .possible_values(&["build-and-test",
                                        "build-only",
                                        "check-only",
                                        "unstable-featureS"]))
                .arg(Arg::with_name("check-only")
                     .long("check-only")
                     .required(false)
                     .takes_value(false))
                .arg(Arg::with_name("demo")
                     .long("demo")
                     .required(false)
                     .takes_value(false)))
        .subcommand(
            SubCommand::with_name("prepare-ex")
                .about("prepare shared and local data for experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("copy-ex")
                .about("copy all data from one experiment to another")
                .arg(Arg::with_name("ex1")
                     .required(true)
                     .takes_value(true))
                .arg(Arg::with_name("ex2")
                     .required(true)
                     .takes_value(true)))
        .subcommand(
            SubCommand::with_name("delete-ex")
                .about("delete shared data for experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))

        // Global experiment prep
        .subcommand(
            SubCommand::with_name("prepare-ex-shared")
                .about("prepare shared data for experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("fetch-gh-mirrors")
                .about("fetch github repos for experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("capture-shas")
                .about("TODO")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("download-crates")
                .about("downloads crates to local disk")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("frob-cargo-tomls")
                .about("frobs tomls for experiment crates")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("capture-lockfiles")
                .about("TODO")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default"))
                .arg(Arg::with_name("toolchain")
                     .long("toolchain")
                     .required(false)
                     .takes_value(true)
                     .default_value("stable"))
                .arg(Arg::with_name("all")
                     .long("all")))


        // Local experiment prep
        .subcommand(
            SubCommand::with_name("prepare-ex-local")
                .about("prepare local data for experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("fetch-deps")
                .about("fetch the deps needed for an experiment")
                .arg(Arg::with_name("toolchain")
                     .long("toolchain")
                     .required(true)
                     .takes_value(true)
                     .default_value("stable"))
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
           SubCommand::with_name("prepare-all-toolchains-for-ex")
                .about("prepare all toolchains for local experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
           SubCommand::with_name("delete-all-target-dirs-for-ex")
                .about("delete the cargo target dirs for an experiment")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))

        // Experimenting
        .subcommand(
            SubCommand::with_name("run")
                .about("run an experiment on all toolchains")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("run-tc")
                .about("run an experiment against a single toolchain")
                .arg(Arg::with_name("toolchain")
                     .required(true)
                     .takes_value(true))
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))
        .subcommand(
            SubCommand::with_name("delete-all-results")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))

        // Reporting
        .subcommand(
           SubCommand::with_name("gen-report")
                .about("generate a report")
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default")))

        // Misc
        .subcommand(
            SubCommand::with_name("summarize")
                .about("TODO"))
                .arg(Arg::with_name("ex")
                     .long("ex")
                     .required(false)
                     .default_value("default"))
        .subcommand(
            SubCommand::with_name("sleep")
                .arg(Arg::with_name("secs")
                     .required(true)))


}


// Local prep

fn prepare_local() -> Result<()> {
    toolchain::prepare_toolchain("stable")?;
    docker::build_container()?;

    lists::create_recent_list()?;
    lists::create_second_list()?;
    lists::create_hot_list()?;
    lists::create_all_lists(false)?;

    Ok(())
}

fn prepare_toolchain(m: &ArgMatches) -> Result<()> {
    let ref toolchain = m.value_of("toolchain").expect("");
    toolchain::prepare_toolchain(toolchain)
}

fn build_container() -> Result<()> {
    docker::build_container()
}

// List creation

fn create_lists(m: &ArgMatches) -> Result<()> {
    let full = m.value_of("full").is_some();
    lists::create_all_lists(full)
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

fn create_gh_candidate_list_from_cache() -> Result<()> {
    lists::create_gh_candidate_list_from_cache()
}

fn create_gh_app_list_from_cache() -> Result<()> {
    lists::create_gh_app_list_from_cache()
}


// Experiment prep

fn define_ex(m: &ArgMatches) -> Result<()> {
    use ex::*;

    let ref ex_name = m.value_of("ex").expect("");
    let toolchains = m.values_of("toolchain").expect("").collect::<Vec<_>>();
    let demo = m.is_present("demo");
    let type_ = m.value_of("type").expect("");

    let mut tcs = Vec::new();
    for tc in toolchains {
        tcs.push(toolchain::parse_toolchain(tc)?);
    }

    let mode = match type_ {
        "build-and-test" => ExMode::BuildAndTest,
        "build-only" => ExMode::BuildOnly,
        "check-only" => ExMode::CheckOnly,
        "unstable-features" => ExMode::UnstableFeatures,
        _ => panic!()
    };

    let opts = ExOpts {
        name: ex_name.to_string(),
        toolchains: tcs,
        mode: mode,
        crates: if demo { ExCrateSelect::Demo } else { ExCrateSelect:: Default },
    };

    ex::define(opts)?;

    Ok(())
}

fn prepare_ex(m: &ArgMatches) -> Result<()> {
    prepare_ex_shared(m)?;
    prepare_ex_local(m)?;

    Ok(())
}

fn copy_ex(m: &ArgMatches) -> Result<()> {
    let ref ex1_name = m.value_of("ex1").expect("");
    let ref ex2_name = m.value_of("ex2").expect("");
    ex::copy(ex1_name, ex2_name)?;

    Ok(())
}

fn delete_ex(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::delete(ex_name)?;

    Ok(())
}


// Global experiment prep

fn prepare_ex_shared(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::fetch_gh_mirrors(ex_name)?;
    ex::capture_shas(ex_name)?;
    ex::download_crates(ex_name)?;
    ex::frob_tomls(ex_name)?;
    ex::capture_lockfiles(ex_name, "stable", false)?;

    Ok(())
}

fn fetch_gh_mirrors(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::fetch_gh_mirrors(ex_name)
}

fn capture_shas(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::capture_shas(ex_name)
}

fn download_crates(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::download_crates(ex_name)
}

fn frob_cargo_tomls(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::frob_tomls(ex_name)
}

fn capture_lockfiles(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    let ref toolchain = m.value_of("toolchain").expect("");
    let all = m.is_present("all");
    ex::capture_lockfiles(ex_name, toolchain, all)
}


// Local experiment prep

fn prepare_ex_local(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::delete_all_target_dirs(ex_name)?;
    ex::fetch_deps(ex_name, "stable")?;
    ex::prepare_all_toolchains(ex_name)?;

    Ok(())
}

fn fetch_deps(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    let ref toolchain = m.value_of("toolchain").expect("");
    ex::fetch_deps(ex_name, toolchain)
}

fn prepare_all_toolchains_for_ex(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::prepare_all_toolchains(ex_name)
}

fn delete_all_target_dirs_for_ex(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex::delete_all_target_dirs(ex_name)
}

// Experiment running

fn run(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex_run::run_ex_all_tcs(ex_name)
}

fn run_tc(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    let ref toolchain = m.value_of("toolchain").expect("");
    ex_run::run_ex(ex_name, toolchain)
}

fn delete_all_results(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    ex_run::delete_all_results(ex_name)
}


// Reporting

fn gen_report(m: &ArgMatches) -> Result<()> {
    let ref ex_name = m.value_of("ex").expect("");
    report::gen(ex_name)
}


// Other

fn sleep(m: &ArgMatches) -> Result<()> {
    let ref secs = m.value_of("secs").expect("");
    run::run("sleep", &[secs], &[]);
    Ok(())
}
