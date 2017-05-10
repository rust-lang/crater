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
use job::JobId;

// An experiment name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ex(String);

// A toolchain name, either a rustup channel identifier,
// or a URL+branch+sha: https://github.com/rust-lang/rust+master+sha
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tc(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SayMsg(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job(pub JobId);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Cmd {
    /* Basic synchronous commands */

    // Local prep
    PrepareLocal,
    PrepareToolchain(Tc),
    BuildContainer,

    // List creation
    CreateLists,
    CreateListsFull,
    CreateRecentList,
    CreateSecondList,
    CreateHotList,
    CreatePopList,
    CreateGhCandidateList,
    CreateGhAppList,
    CreateGhCandidateListFromCache,
    CreateGhAppListFromCache,

    // Master experiment prep
    DefineEx(Ex, Tc, Tc, ExMode, ExCrateSelect),
    PrepareEx(Ex),
    CopyEx(Ex, Ex),
    DeleteEx(Ex),

    // Shared experiment prep
    PrepareExShared(Ex),
    FetchGhMirrors(Ex),
    CaptureShas(Ex),
    DownloadCrates(Ex),
    FrobCargoTomls(Ex),
    CaptureLockfiles(Ex, Tc),

    // Local experiment prep
    PrepareExLocal(Ex),
    DeleteAllTargetDirs(Ex),
    DeleteAllResults(Ex),
    FetchDeps(Ex, Tc),
    PrepareAllToolchains(Ex),

    // Experimenting
    Run(Ex),
    RunTc(Ex, Tc),

    // Reporting
    GenReport(Ex),

    // Job control
    CreateDockerJob(Box<Cmd>),
    StartJob(Job),
    WaitForJob(Job),
    RunJob(Job),
    RunJobAgain(Job),
    RunCmdForJob(Job),

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

use self::state::GlobalState;
use bmk::Process;

impl Process<GlobalState> for Cmd {
    fn process(self, st: GlobalState) -> Result<(GlobalState, Vec<Cmd>)> {
        use lists;
        use toolchain;
        use docker;
        use ex;
        use ex_run;
        use run;
        use report;
        use job;

        let mut cmds = Vec::new();
        match self {
            // Local prep
            Cmd::PrepareLocal => {
                cmds.extend(vec![
                    Cmd::PrepareToolchain("stable".parse::<Tc>()?),
                    Cmd::BuildContainer,
                    Cmd::CreateLists,
                ]);
            }
            Cmd::PrepareToolchain(tc) => tc.0.parse::<toolchain::Toolchain>()?.prepare()?,
            Cmd::BuildContainer => docker::build_container()?,

            // List creation
            Cmd::CreateLists => {
                cmds.extend(vec![
                    Cmd::CreateRecentList,
                    Cmd::CreateSecondList,
                    Cmd::CreateHotList,
                    Cmd::CreatePopList,
                    Cmd::CreateGhCandidateListFromCache,
                    Cmd::CreateGhAppListFromCache,
                ]);
            }
            Cmd::CreateListsFull => {
                cmds.extend(vec![
                    Cmd::CreateRecentList,
                    Cmd::CreateSecondList,
                    Cmd::CreateHotList,
                    Cmd::CreatePopList,
                    Cmd::CreateGhCandidateList,
                    Cmd::CreateGhAppList,
                ]);
            }
            Cmd::CreateRecentList => lists::create_recent_list()?,
            Cmd::CreateSecondList => lists::create_second_list()?,
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
                               toolchains: vec![
                    tc1.0.parse::<toolchain::Toolchain>()?,
                    tc2.0.parse::<toolchain::Toolchain>()?,
                ],
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
                    Cmd::CaptureLockfiles(ex, "stable".parse::<Tc>()?),
                ]);
            }
            Cmd::FetchGhMirrors(ex) => ex::fetch_gh_mirrors(&ex.0)?,
            Cmd::CaptureShas(ex) => ex::capture_shas(&ex.0)?,
            Cmd::DownloadCrates(ex) => ex::download_crates(&ex.0)?,
            Cmd::FrobCargoTomls(ex) => ex::frob_tomls(&ex.0)?,
            Cmd::CaptureLockfiles(ex, tc) => ex::capture_lockfiles(&ex.0, &tc.0, false)?,

            // Local experiment prep
            Cmd::PrepareExLocal(ex) => {
                cmds.extend(vec![
                    Cmd::DeleteAllTargetDirs(ex.clone()),
                    Cmd::DeleteAllResults(ex.clone()),
                    Cmd::FetchDeps(ex.clone(), "stable".parse::<Tc>()?),
                    Cmd::PrepareAllToolchains(ex),
                ]);
            }
            Cmd::DeleteAllTargetDirs(ex) => ex::delete_all_target_dirs(&ex.0)?,
            Cmd::DeleteAllResults(ex) => ex_run::delete_all_results(&ex.0)?,
            Cmd::FetchDeps(ex, tc) => ex::fetch_deps(&ex.0, &tc.0)?,
            Cmd::PrepareAllToolchains(ex) => ex::prepare_all_toolchains(&ex.0)?,

            // Experimenting
            Cmd::Run(ex) => ex_run::run_ex_all_tcs(&ex.0)?,
            Cmd::RunTc(ex, tc) => ex_run::run_ex(&ex.0, &tc.0)?,

            // Reporting
            Cmd::GenReport(ex) => report::gen(&ex.0)?,

            // Job control
            Cmd::CreateDockerJob(cmd) => job::create_local(*cmd)?,
            Cmd::StartJob(job) => job::start(job.0)?,
            Cmd::WaitForJob(job) => job::wait(job.0)?,
            Cmd::RunJob(job) => job::run(job.0)?,
            Cmd::RunJobAgain(job) => job::run_again(job.0)?,
            Cmd::RunCmdForJob(job) => job::run_cmd_for_job(job.0)?,

            // Misc
            Cmd::Sleep => run::run("sleep", &["5"], &[])?,
            Cmd::Say(msg) => log!("{}", msg.0),
        }

        Ok((st, cmds))
    }
}

// Boilerplate conversions on the model. Ideally all this would be generated.
pub mod conv {
    use super::*;

    use bmk::{CmdArg, CmdDesc};
    use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
    use std::str::FromStr;

    pub fn cmd_descs() -> Vec<CmdDesc> {
        vec![
            ("prepare-local", vec![]),
            ("prepare-toolchain", vec![CmdArg::Opt("ex", "default")]),
            ("build-container", vec![]),
            ("create-lists", vec![]),
            ("create-lists-full", vec![]),
            ("create-recent-list", vec![]),
            ("create-second-list", vec![]),
            ("create-hot-list", vec![]),
            ("create-gh-candidate-list", vec![]),
            ("create-gh-app-list", vec![]),
            ("create-gh-candidate-list-from-cache", vec![]),
            ("create-gh-app-list-from-cache", vec![]),
        ]
                .into_iter()
                .map(|(a, b)| CmdDesc { name: a, args: b })
                .collect()
    }

    pub fn clap_cmd<'a>(desc: &CmdDesc) -> App<'a, 'a> {
        fn opt(n: &'static str, def: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n).required(false).long(n).default_value(def)
        }

        fn req(n: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n).required(true)
        }

        fn cmd(n: &'static str, desc: &'static str) -> App<'static, 'static> {
            SubCommand::with_name(n).about(desc)
        }

        let arg_to_str = |cmdarg: &CmdArg| match *cmdarg {
            CmdArg::Req(s) |
            CmdArg::Opt(s, _) => s,
        };

        let args = desc.args
            .iter()
            .map(|arg| match *arg {
                     CmdArg::Req(s) => req(s),
                     CmdArg::Opt(s, d) => opt(s, d),
                 })
            .collect::<Vec<_>>();

        cmd(desc.name, "todo").args(&args)
    }

    pub fn clap_cmds() -> Vec<App<'static, 'static>> {
        clap_cmds_(true)
    }

    fn clap_cmds_(recurse: bool) -> Vec<App<'static, 'static>> {
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
        let job = || req("job");
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
            cmd("create-second-list",
                "create the list of of second-most-recent crate versions"),
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

            // Job control
            if recurse {
                cmd("create-docker-job", "start a docker job in docker")
                    .subcommands(clap_cmds_(false))
            } else {
                cmd("create-docker-job", "nop")
            },
            cmd("start-job", "start a job asynchronously").arg(job()),
            cmd("wait-for-job", "wait for a job to complete").arg(job()),
            cmd("run-job", "run a pending job synchronously").arg(job()),
            cmd("run-job-again", "run a completed job again synchronously").arg(job()),
            cmd("run-cmd-for-job",
                "run a command for a job, inside the job environment")
                    .arg(job()),

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

        fn tc(m: &ArgMatches) -> Result<Tc> {
            m.value_of("tc").expect("").parse::<Tc>()
        }

        fn tc1(m: &ArgMatches) -> Result<Tc> {
            m.value_of("tc-1").expect("").parse::<Tc>()
        }

        fn tc2(m: &ArgMatches) -> Result<Tc> {
            m.value_of("tc-2").expect("").parse::<Tc>()
        }

        fn mode(m: &ArgMatches) -> Result<ExMode> {
            m.value_of("mode").expect("").parse::<ExMode>()
        }

        fn crate_select(m: &ArgMatches) -> Result<ExCrateSelect> {
            m.value_of("crate-select")
                .expect("")
                .parse::<ExCrateSelect>()
        }

        fn cmd(m: &ArgMatches) -> Result<Box<Cmd>> {
            Ok(Box::new(clap_args_to_cmd(m)?))
        }

        fn job(m: &ArgMatches) -> Result<Job> {
            m.value_of("job").expect("").parse::<Job>()
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
               ("create-second-list", _) => Cmd::CreateSecondList,
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

               // Job control
               ("create-docker-job", Some(m)) => Cmd::CreateDockerJob(cmd(m)?),
               ("start-job", Some(m)) => Cmd::StartJob(job(m)?),
               ("wait-for-job", Some(m)) => Cmd::WaitForJob(job(m)?),
               ("run-job", Some(m)) => Cmd::RunJob(job(m)?),
               ("run-job-again", Some(m)) => Cmd::RunJobAgain(job(m)?),
               ("run-cmd-for-job", Some(m)) => Cmd::RunCmdForJob(job(m)?),

               // Misc
               ("sleep", _) => Cmd::Sleep,
               ("say", Some(m)) => Cmd::Say(say_msg(m)?),

               (s, _) => panic!("unimplemented args_to_cmd {}", s),
           })
    }

    fn cmd_to_name(cmd: &Cmd) -> &'static str {
        use super::Cmd::*;
        match *cmd {
            PrepareLocal => "prepare-local",
            PrepareToolchain(..) => "prepare-toolchain",
            BuildContainer => "build-container",

            CreateLists => "create-lists",
            CreateListsFull => "create-lists-full",
            CreateRecentList => "create-recent-list",
            CreateSecondList => "create-second-list",
            CreateHotList => "create-hot-list",
            CreatePopList => "create-pop-list",
            CreateGhCandidateList => "create-gh-candidate-list",
            CreateGhAppList => "create-gh-app-list",
            CreateGhCandidateListFromCache => "create-gh-candidate-list-from-cache",
            CreateGhAppListFromCache => "create-gh-app-list-from-cache",

            DefineEx(..) => "define-ex",
            PrepareEx(..) => "prepare-ex",
            CopyEx(..) => "copy-ex",
            DeleteEx(..) => "delete-ex",

            PrepareExShared(..) => "prepare-ex-shared",
            FetchGhMirrors(..) => "fetch-gh-mirrors",
            CaptureShas(..) => "capture-shas",
            DownloadCrates(..) => "download-crates",
            FrobCargoTomls(..) => "frob-cargo-tomls",
            CaptureLockfiles(..) => "capture-lockfiles",

            PrepareExLocal(..) => "prepare-ex-local",
            DeleteAllTargetDirs(..) => "delete-all-target-dirs",
            DeleteAllResults(..) => "delete-all-results",
            FetchDeps(..) => "fetch-deps",
            PrepareAllToolchains(..) => "prepare-all-toolchains",

            Run(..) => "run",
            RunTc(..) => "run-tc",

            GenReport(..) => "gen-report",

            CreateDockerJob(..) => "create-docker-job",
            StartJob(..) => "start-job",
            WaitForJob(..) => "wait-for-job",
            RunJob(..) => "run-job",
            RunJobAgain(..) => "run-job-again",
            RunCmdForJob(..) => "run-cmd-for-job",

            Sleep => "sleep",
            Say(..) => "say",
        }
    }

    pub fn cmd_to_args(cmd: Cmd) -> Vec<String> {
        Some(cmd_to_name(&cmd))
            .into_iter()
            .map(|s| s.to_string())
            .chain(cmd_to_args_(cmd).into_iter())
            .collect()
    }

    fn cmd_to_args_(cmd: Cmd) -> Vec<String> {
        use super::Cmd::*;

        fn opt_ex(ex: Ex) -> String {
            format!("--ex={}", ex.0)
        }

        fn req_ex(ex: Ex) -> String {
            format!("{}", ex.0)
        }

        fn opt_tc(tc: Tc) -> String {
            format!("--tc={}", tc.0)
        }

        fn req_tc(tc: Tc) -> String {
            tc.0
        }

        fn opt_mode(mode: ExMode) -> String {
            format!("--mode={}", mode.to_str())
        }

        fn opt_crate_select(crate_select: ExCrateSelect) -> String {
            format!("--crate-select={}", crate_select.to_str())
        }

        fn req_job(job: Job) -> String {
            job.0.to_string()
        }

        fn req_say_msg(say_msg: SayMsg) -> String {
            say_msg.0
        }

        match cmd {
            PrepareLocal |
            BuildContainer |
            CreateLists |
            CreateListsFull |
            CreateRecentList |
            CreateSecondList |
            CreateHotList |
            CreatePopList |
            CreateGhCandidateList |
            CreateGhAppList |
            CreateGhCandidateListFromCache |
            CreateGhAppListFromCache |
            Sleep => vec![],

            PrepareToolchain(tc) => vec![req_tc(tc)],
            DefineEx(ex, tc1, tc2, mode, crate_select) => {
                vec![
                    opt_ex(ex),
                    req_tc(tc1),
                    req_tc(tc2),
                    opt_mode(mode),
                    opt_crate_select(crate_select),
                ]
            }
            PrepareEx(ex) => vec![opt_ex(ex)],
            CopyEx(ex1, ex2) => vec![req_ex(ex1), req_ex(ex2)],

            DeleteEx(ex) => vec![opt_ex(ex)],
            PrepareExShared(ex) => vec![opt_ex(ex)],
            FetchGhMirrors(ex) => vec![opt_ex(ex)],
            CaptureShas(ex) => vec![opt_ex(ex)],
            DownloadCrates(ex) => vec![opt_ex(ex)],
            FrobCargoTomls(ex) => vec![opt_ex(ex)],
            CaptureLockfiles(ex, tc) => vec![opt_ex(ex), opt_tc(tc)],

            PrepareExLocal(ex) => vec![opt_ex(ex)],
            DeleteAllTargetDirs(ex) => vec![opt_ex(ex)],
            DeleteAllResults(ex) => vec![opt_ex(ex)],
            FetchDeps(ex, tc) => vec![opt_ex(ex), opt_tc(tc)],
            PrepareAllToolchains(ex) => vec![opt_ex(ex)],

            Run(ex) => vec![opt_ex(ex)],
            RunTc(ex, tc) => vec![opt_ex(ex), req_tc(tc)],

            GenReport(ex) => vec![opt_ex(ex)],

            CreateDockerJob(cmd) => cmd_to_args(*cmd),
            StartJob(job) => vec![req_job(job)],
            WaitForJob(job) => vec![req_job(job)],
            RunJob(job) => vec![req_job(job)],
            RunJobAgain(job) => vec![req_job(job)],
            RunCmdForJob(job) => vec![req_job(job)],

            Say(msg) => vec![req_say_msg(msg)],
        }
    }

    pub fn args_to_cmd(args: Vec<String>) -> Result<Cmd> {
        let m = App::new("")
            .setting(AppSettings::NoBinaryName)
            .subcommands(clap_cmds())
            .get_matches_from(&args);
        clap_args_to_cmd(&m)
    }

    use bmk::Arguable;

    impl Arguable for Cmd {
        fn from_args(args: Vec<String>) -> Result<Self> {
            args_to_cmd(args)
        }

        fn to_args(self) -> Vec<String> {
            cmd_to_args(self)
        }
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

    impl FromStr for Job {
        type Err = Error;

        fn from_str(job: &str) -> Result<Job> {
            Ok(Job(JobId(job.parse().chain_err(|| "parsing job id")?)))
        }
    }

    impl FromStr for Ex {
        type Err = Error;

        fn from_str(ex: &str) -> Result<Ex> {
            Ok(Ex(ex.to_string()))
        }
    }

    impl FromStr for Tc {
        type Err = Error;

        fn from_str(tc: &str) -> Result<Tc> {
            use toolchain;
            let _ = tc.parse::<toolchain::Toolchain>()?;
            Ok(Tc(tc.to_string()))
        }
    }
}

pub mod state {
    use super::slowio::{Blobject, FreeDir};

    pub struct GlobalState {
        master: MasterState,
        local: LocalState,
        shared: SharedState,
        ex: ExData,
    }

    pub struct MasterState;

    pub struct LocalState {
        cargo_home: FreeDir,
        rustup_home: FreeDir,
        crates_io_index_mirror: FreeDir,
        gh_clones: FreeDir,
        target_dirs: FreeDir,
        test_source_dir: FreeDir,
    }

    pub struct SharedState {
        crates: FreeDir,
        gh_mirrors: FreeDir,
        lists: Lists,
    }

    pub struct Lists {
        recent: Blobject,
        second: Blobject,
        hot: Blobject,
        gh_repos: Blobject,
        gh_apps: Blobject,
    }

    pub struct ExData {
        config: Blobject,
    }

    impl GlobalState {
        pub fn init() -> GlobalState {
            GlobalState {
                master: MasterState,
                local: LocalState {
                    cargo_home: FreeDir,
                    rustup_home: FreeDir,
                    crates_io_index_mirror: FreeDir,
                    gh_clones: FreeDir,
                    target_dirs: FreeDir,
                    test_source_dir: FreeDir,
                },
                shared: SharedState {
                    crates: FreeDir,
                    gh_mirrors: FreeDir,
                    lists: Lists {
                        recent: Blobject,
                        second: Blobject,
                        hot: Blobject,
                        gh_repos: Blobject,
                        gh_apps: Blobject,
                    },
                },
                ex: ExData { config: Blobject },
            }
        }
    }
}

pub mod slowio {
    #[derive(Default)]
    pub struct FreeDir;
    #[derive(Default)]
    pub struct Blobject;
}
