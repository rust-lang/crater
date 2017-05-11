/*!

Cargobomb works by serially processing a queue of commands, each of
which transforms the application state in some discrete way, and
designed to be resilient to I/O errors. The application state is
backed by a directory in the filesystem, and optionally synchronized
with s3.

These command queues may be created dynamically and executed in
parallel jobs, either locally, or distributed on e.g. AWS. The
application state employs ownership techniques to ensure that
parallel access is consistent and race-free.

NB: The design of this module is SERIOUSLY MESSED UP, with lots of
duplication, the result of a deep yak shave that failed. It needs a
rewrite.

*/

use errors::*;
use toolchain::Toolchain;

// An experiment name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ex(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SayMsg(String);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Cmd {
    /* Basic synchronous commands */

    // Local prep
    PrepareLocal,
    PrepareToolchain(Toolchain),
    BuildContainer,

    // List creation
    CreateLists,
    CreateListsFull,
    CreateRecentList,
    CreateHotList,
    CreatePopList,
    CreateGhCandidateList,
    CreateGhAppList,
    CreateGhCandidateListFromCache,
    CreateGhAppListFromCache,

    // Master experiment prep
    DefineEx(Ex, Toolchain, Toolchain, ExMode, ExCrateSelect),
    PrepareEx(Ex),
    CopyEx(Ex, Ex),
    DeleteEx(Ex),

    // Shared experiment prep
    PrepareExShared(Ex),
    FetchGhMirrors(Ex),
    CaptureShas(Ex),
    DownloadCrates(Ex),
    FrobCargoTomls(Ex),
    CaptureLockfiles(Ex, Toolchain),

    // Local experiment prep
    PrepareExLocal(Ex),
    DeleteAllTargetDirs(Ex),
    DeleteAllResults(Ex),
    FetchDeps(Ex, Toolchain),
    PrepareAllToolchains(Ex),

    // Experimenting
    Run(Ex),
    RunTc(Ex, Toolchain),

    // Reporting
    GenReport(Ex),

    // Misc
    Sleep,
    Say(SayMsg),
}

#[derive(Serialize, Deserialize)]
#[derive(Debug, Clone)]
pub enum ExMode {
    BuildAndTest,
    BuildOnly,
    CheckOnly,
    UnstableFeatures,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ExCrateSelect {
    Full,
    Demo,
    SmallRandom,
    Top100,
}

use bmk::Process;

impl Process for Cmd {
    fn process(self) -> Result<Vec<Cmd>> {
        use lists;
        use docker;
        use ex;
        use ex_run;
        use run;
        use report;

        let mut cmds = Vec::new();
        match self {
            // Local prep
            Cmd::PrepareLocal => {
                cmds.extend(vec![
                    Cmd::PrepareToolchain("stable".parse()?),
                    Cmd::BuildContainer,
                    Cmd::CreateLists,
                ]);
            }
            Cmd::PrepareToolchain(tc) => tc.prepare()?,
            Cmd::BuildContainer => docker::build_container()?,

            // List creation
            Cmd::CreateLists => {
                cmds.extend(vec![
                    Cmd::CreateRecentList,
                    Cmd::CreateHotList,
                    Cmd::CreatePopList,
                    Cmd::CreateGhCandidateListFromCache,
                    Cmd::CreateGhAppListFromCache,
                ]);
            }
            Cmd::CreateListsFull => {
                cmds.extend(vec![
                    Cmd::CreateRecentList,
                    Cmd::CreateHotList,
                    Cmd::CreatePopList,
                    Cmd::CreateGhCandidateList,
                    Cmd::CreateGhAppList,
                ]);
            }
            Cmd::CreateRecentList => lists::create_recent_list()?,
            Cmd::CreateHotList => lists::create_hot_list()?,
            Cmd::CreatePopList => lists::create_pop_list()?,
            Cmd::CreateGhCandidateList => lists::create_gh_candidate_list()?,
            Cmd::CreateGhAppList => lists::create_gh_app_list()?,
            Cmd::CreateGhCandidateListFromCache => lists::create_gh_candidate_list_from_cache()?,
            Cmd::CreateGhAppListFromCache => lists::create_gh_app_list_from_cache()?,

            // Experiment prep
            Cmd::DefineEx(ex, tc1, tc2, mode, crates) => {
                ex::define(ex::ExOpts {
                               name: ex.0,
                               toolchains: vec![tc1, tc2],
                               mode: mode,
                               crates: crates,
                           })?;
            }
            Cmd::PrepareEx(ex) => {
                cmds.extend(vec![Cmd::PrepareExShared(ex.clone()), Cmd::PrepareExLocal(ex)]);
            }
            Cmd::CopyEx(ex1, ex2) => ex::copy(&ex1.0, &ex2.0)?,
            Cmd::DeleteEx(ex) => ex::delete(&ex.0)?,

            // Shared emperiment prep
            Cmd::PrepareExShared(ex) => {
                cmds.extend(vec![
                    Cmd::FetchGhMirrors(ex.clone()),
                    Cmd::CaptureShas(ex.clone()),
                    Cmd::DownloadCrates(ex.clone()),
                    Cmd::FrobCargoTomls(ex.clone()),
                    Cmd::CaptureLockfiles(ex, "stable".parse()?),
                ]);
            }
            Cmd::FetchGhMirrors(ex) => ex::fetch_gh_mirrors(&ex.0)?,
            Cmd::CaptureShas(ex) => ex::capture_shas(&ex.0)?,
            Cmd::DownloadCrates(ex) => ex::download_crates(&ex.0)?,
            Cmd::FrobCargoTomls(ex) => ex::frob_tomls(&ex.0)?,
            Cmd::CaptureLockfiles(ex, tc) => ex::capture_lockfiles(&ex.0, &tc, false)?,

            // Local experiment prep
            Cmd::PrepareExLocal(ex) => {
                cmds.extend(vec![
                    Cmd::DeleteAllTargetDirs(ex.clone()),
                    Cmd::DeleteAllResults(ex.clone()),
                    Cmd::FetchDeps(ex.clone(), "stable".parse()?),
                    Cmd::PrepareAllToolchains(ex),
                ]);
            }
            Cmd::DeleteAllTargetDirs(ex) => ex::delete_all_target_dirs(&ex.0)?,
            Cmd::DeleteAllResults(ex) => ex_run::delete_all_results(&ex.0)?,
            Cmd::FetchDeps(ex, tc) => ex::fetch_deps(&ex.0, &tc)?,
            Cmd::PrepareAllToolchains(ex) => ex::prepare_all_toolchains(&ex.0)?,

            // Experimenting
            Cmd::Run(ex) => ex_run::run_ex_all_tcs(&ex.0)?,
            Cmd::RunTc(ex, tc) => ex_run::run_ex(&ex.0, tc)?,

            // Reporting
            Cmd::GenReport(ex) => report::gen(&ex.0)?,

            // Misc
            Cmd::Sleep => run::run("sleep", &["5"], &[])?,
            Cmd::Say(msg) => log!("{}", msg.0),
        }

        Ok(cmds)
    }
}

// Boilerplate conversions on the model. Ideally all this would be generated.
pub mod conv {
    use super::*;

    use clap::{App, Arg, ArgMatches, SubCommand};
    use std::str::FromStr;

    pub fn clap_cmds() -> Vec<App<'static, 'static>> {
        // Types of arguments
        let ex = || opt("ex", "default");
        let ex1 = || req("ex-1");
        let ex2 = || req("ex-2");
        let req_tc = || req("tc");
        let opt_tc = || opt("tc", "stable");
        let tc1 = || req("tc-1");
        let tc2 = || req("tc-2");
        let mode = || {
            Arg::with_name("mode")
                .required(false)
                .long("mode")
                .default_value(ExMode::BuildAndTest.to_str())
                .possible_values(&[
                    ExMode::BuildAndTest.to_str(),
                    ExMode::BuildOnly.to_str(),
                    ExMode::CheckOnly.to_str(),
                    ExMode::UnstableFeatures.to_str(),
                ])
        };
        let crate_select = || {
            Arg::with_name("crate-select")
                .required(false)
                .long("crate-select")
                .default_value(ExCrateSelect::Demo.to_str())
                .possible_values(&[
                    ExCrateSelect::Demo.to_str(),
                    ExCrateSelect::Full.to_str(),
                    ExCrateSelect::SmallRandom.to_str(),
                    ExCrateSelect::Top100.to_str(),
                ])
        };
        let say_msg = || req("say-msg");

        fn opt(n: &'static str, def: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n).required(false).long(n).default_value(def)
        }

        fn req(n: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n).required(true)
        }

        fn cmd(n: &'static str, desc: &'static str) -> App<'static, 'static> {
            SubCommand::with_name(n).about(desc)
        }

        vec![
            // Local prep
            cmd("prepare-local",
                "acquire toolchains, build containers, build crate lists"),
            cmd("prepare-toolchain", "install or update a toolchain").arg(req_tc()),
            cmd("build-container",
                "build docker container needed by experiments"),

            // List creation
            cmd("create-lists", "create all the lists of crates"),
            cmd("create-lists-full", "create all the lists of crates"),
            cmd("create-recent-list",
                "create the list of most recent crate versions"),
            cmd("create-hot-list",
                "create the list of popular crates versions"),
            cmd("create-pop-list", "create the list of popular crates"),
            cmd("create-gh-candidate-list",
                "crate the list of all GitHub Rust repos"),
            cmd("create-gh-app-list",
                "create the list of GitHub Rust applications"),
            cmd("create-gh-candidate-list-from-cache",
                "crate the list of all GitHub Rust repos from cache"),
            cmd("create-gh-app-list-from-cache",
                "create the list of GitHub Rust applications from cache"),

            // Master experiment prep
            cmd("define-ex", "define an experiment")
                .arg(ex())
                .arg(tc1())
                .arg(tc2())
                .arg(mode())
                .arg(crate_select()),
            cmd("prepare-ex", "prepare shared and local data for experiment").arg(ex()),
            cmd("copy-ex", "copy all data from one experiment to another")
                .arg(ex1())
                .arg(ex2()),
            cmd("delete-ex", "delete shared data for experiment").arg(ex()),

            // Shared experiment prep
            cmd("prepare-ex-shared", "prepare shared data for experiment").arg(ex()),
            cmd("fetch-gh-mirrors", "fetch github repos for experiment").arg(ex()),
            cmd("capture-shas", "record the head commits of GitHub repos").arg(ex()),
            cmd("download-crates", "download crates to local disk").arg(ex()),
            cmd("frob-cargo-tomls", "frobsm tomls for experiment crates").arg(ex()),
            cmd("capture-lockfiles",
                "records lockfiles for all crates in experiment")
                    .arg(ex())
                    .arg(opt_tc()),

            // Local experiment prep
            cmd("prepare-ex-local", "prepare local data for experiment").arg(ex()),
            cmd("delete-all-target-dirs",
                "delete the cargo target dirs for an experiment")
                    .arg(ex()),
            cmd("delete-all-results", "delete all results for an experiment").arg(ex()),
            cmd("fetch-deps", "fetch all dependencies for an experiment")
                .arg(ex())
                .arg(opt_tc()),
            cmd("prepare-all-toolchains",
                "prepare all toolchains for local experiment")
                    .arg(ex()),

            // Experimenting
            cmd("run", "run an experiment, with all toolchains").arg(ex()),
            cmd("run-tc", "run an experiment, with a single toolchain")
                .arg(ex())
                .arg(req_tc()),

            // Reporting
            cmd("gen-report", "generate the experiment report").arg(ex()),

            // Misc
            cmd("sleep", "sleep"),
            cmd("say", "say something").arg(say_msg()),
        ]
    }

    pub fn clap_args_to_cmd(m: &ArgMatches) -> Result<Cmd> {

        fn ex(m: &ArgMatches) -> Result<Ex> {
            m.value_of("ex").expect("").parse::<Ex>()
        }

        fn ex1(m: &ArgMatches) -> Result<Ex> {
            m.value_of("ex-1").expect("").parse::<Ex>()
        }

        fn ex2(m: &ArgMatches) -> Result<Ex> {
            m.value_of("ex-2").expect("").parse::<Ex>()
        }

        fn tc(m: &ArgMatches) -> Result<Toolchain> {
            m.value_of("tc").expect("").parse()
        }

        fn tc1(m: &ArgMatches) -> Result<Toolchain> {
            m.value_of("tc-1").expect("").parse()
        }

        fn tc2(m: &ArgMatches) -> Result<Toolchain> {
            m.value_of("tc-2").expect("").parse()
        }

        fn mode(m: &ArgMatches) -> Result<ExMode> {
            m.value_of("mode").expect("").parse::<ExMode>()
        }

        fn crate_select(m: &ArgMatches) -> Result<ExCrateSelect> {
            m.value_of("crate-select")
                .expect("")
                .parse::<ExCrateSelect>()
        }

        fn say_msg(m: &ArgMatches) -> Result<SayMsg> {
            Ok(SayMsg(m.value_of("say-msg").expect("").to_string()))
        }

        Ok(match m.subcommand() {
               // Local prep
               ("prepare-local", _) => Cmd::PrepareLocal,
               ("prepare-toolchain", Some(m)) => Cmd::PrepareToolchain(tc(m)?),
               ("build-container", _) => Cmd::BuildContainer,

               // List creation
               ("create-lists", _) => Cmd::CreateLists,
               ("create-lists-full", _) => Cmd::CreateListsFull,
               ("create-recent-list", _) => Cmd::CreateRecentList,
               ("create-hot-list", _) => Cmd::CreateHotList,
               ("create-pop-list", _) => Cmd::CreatePopList,
               ("create-gh-candidate-list", _) => Cmd::CreateGhCandidateList,
               ("create-gh-app-list", _) => Cmd::CreateGhAppList,
               ("create-gh-candidate-list-from-cache", _) => Cmd::CreateGhCandidateListFromCache,
               ("create-gh-app-list-from-cache", _) => Cmd::CreateGhAppListFromCache,

               // Master experiment prep
               ("define-ex", Some(m)) => {
                   Cmd::DefineEx(ex(m)?, tc1(m)?, tc2(m)?, mode(m)?, crate_select(m)?)
               }
               ("prepare-ex", Some(m)) => Cmd::PrepareEx(ex(m)?),
               ("copy-ex", Some(m)) => Cmd::CopyEx(ex1(m)?, ex2(m)?),
               ("delete-ex", Some(m)) => Cmd::DeleteEx(ex(m)?),

               // Shared experiment prep
               ("prepare-ex-shared", Some(m)) => Cmd::PrepareExShared(ex(m)?),
               ("fetch-gh-mirrors", Some(m)) => Cmd::FetchGhMirrors(ex(m)?),
               ("capture-shas", Some(m)) => Cmd::CaptureShas(ex(m)?),
               ("download-crates", Some(m)) => Cmd::DownloadCrates(ex(m)?),
               ("frob-cargo-tomls", Some(m)) => Cmd::FrobCargoTomls(ex(m)?),
               ("capture-lockfiles", Some(m)) => Cmd::CaptureLockfiles(ex(m)?, tc(m)?),

               // Local experiment prep
               ("prepare-ex-local", Some(m)) => Cmd::PrepareExLocal(ex(m)?),
               ("delete-all-target-dirs", Some(m)) => Cmd::DeleteAllTargetDirs(ex(m)?),
               ("delete-all-results", Some(m)) => Cmd::DeleteAllResults(ex(m)?),
               ("fetch-deps", Some(m)) => Cmd::FetchDeps(ex(m)?, tc(m)?),
               ("prepare-all-toolchains", Some(m)) => Cmd::PrepareAllToolchains(ex(m)?),

               // Experimenting
               ("run", Some(m)) => Cmd::Run(ex(m)?),
               ("run-tc", Some(m)) => Cmd::RunTc(ex(m)?, tc(m)?),

               // Reporting
               ("gen-report", Some(m)) => Cmd::GenReport(ex(m)?),

               // Misc
               ("sleep", _) => Cmd::Sleep,
               ("say", Some(m)) => Cmd::Say(say_msg(m)?),

               (s, _) => panic!("unimplemented args_to_cmd {}", s),
           })
    }

    impl FromStr for ExMode {
        type Err = Error;

        fn from_str(s: &str) -> Result<ExMode> {
            Ok(match s {
                   "build-and-test" => ExMode::BuildAndTest,
                   "build-only" => ExMode::BuildOnly,
                   "check-only" => ExMode::CheckOnly,
                   "unstable-features" => ExMode::UnstableFeatures,
                   s => bail!("invalid ex-mode: {}", s),
               })
        }
    }

    impl ExMode {
        pub fn to_str(&self) -> &'static str {
            match *self {
                ExMode::BuildAndTest => "build-and-test",
                ExMode::BuildOnly => "build-only",
                ExMode::CheckOnly => "check-only",
                ExMode::UnstableFeatures => "unstable-features",
            }
        }
    }

    impl FromStr for ExCrateSelect {
        type Err = Error;

        fn from_str(s: &str) -> Result<ExCrateSelect> {
            Ok(match s {
                   "full" => ExCrateSelect::Full,
                   "demo" => ExCrateSelect::Demo,
                   "small-random" => ExCrateSelect::SmallRandom,
                   "top-100" => ExCrateSelect::Top100,
                   s => bail!("invalid crate-select: {}", s),
               })
        }
    }

    impl ExCrateSelect {
        pub fn to_str(&self) -> &'static str {
            match *self {
                ExCrateSelect::Full => "full",
                ExCrateSelect::Demo => "demo",
                ExCrateSelect::SmallRandom => "small-random",
                ExCrateSelect::Top100 => "top-100",
            }
        }
    }

    impl FromStr for Ex {
        type Err = Error;

        fn from_str(ex: &str) -> Result<Ex> {
            Ok(Ex(ex.to_string()))
        }
    }
}
