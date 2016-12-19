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

*/

use errors::*;

// An experiment name
#[derive(Debug, Clone)]
pub struct Ex(String);

// A toolchain name, either a rustup channel identifier,
// or a URL+branch+sha: https://github.com/rust-lang/rust+master+sha
#[derive(Debug, Clone)]
pub struct Tc(String);

#[derive(Debug)]
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
    FetchGhMirrors(Ex) ,
    CaptureShas(Ex),
    DownloadCrates(Ex),
    FrobCargoTomls(Ex),
    CaptureLockfiles(Ex, Tc),
    CaptureAllLockfiles(Ex, Tc),

    // Local experiment prep
    PrepareExLocal(Ex),
    FetchDeps(Ex, Tc),
    PrepareAllToolchainsForEx(Ex),
    DeleteAllTargetDirsForEx(Ex),

    // Experimenting
    Run(Ex, Tc),
    DeleteAllResults(Ex),

    // Reporting
    GenReport(Ex),

    // Misc
    LinkToolchain,
    RunUnstableFeatures,
    Summarize,
    EasyTest,
    Sleep,
}

#[derive(Serialize, Deserialize)]
#[derive(Debug)]
pub enum ExMode {
    BuildAndTest,
    BuildOnly,
    CheckOnly,
    UnstableFeatures
}

#[derive(Debug)]
pub enum ExCrateSelect {
    Full,
    Demo,
}

use self::driver::Process;
use self::state::GlobalState;

impl Process<GlobalState> for Cmd {
    fn process(self, st: GlobalState) -> Result<(GlobalState, Vec<Cmd>)> {
        use lists;
        use toolchain;
        use docker;
        use ex;

        let mut cmds = Vec::new();
        match self {
            // Local prep
            Cmd::PrepareLocal => {
                cmds.extend(vec![Cmd::PrepareToolchain(Tc::from_str("stable")?),
                                 Cmd::BuildContainer,
                                 Cmd::CreateLists]);
            }
            Cmd::PrepareToolchain(tc) => toolchain::prepare_toolchain(&tc.0)?,
            Cmd::BuildContainer => docker::build_container()?,

            // List creation
            Cmd::CreateLists => {
                cmds.extend(vec![Cmd::CreateRecentList,
                                 Cmd::CreateSecondList,
                                 Cmd::CreateHotList,
                                 Cmd::CreateGhCandidateListFromCache,
                                 Cmd::CreateGhAppListFromCache]);
            }
            Cmd::CreateListsFull => {
                cmds.extend(vec![Cmd::CreateRecentList,
                                 Cmd::CreateSecondList,
                                 Cmd::CreateHotList,
                                 Cmd::CreateGhCandidateList,
                                 Cmd::CreateGhAppList]);
            }
            Cmd::CreateRecentList => lists::create_recent_list()?,
            Cmd::CreateSecondList => lists::create_second_list()?,
            Cmd::CreateHotList => lists::create_hot_list()?,
            Cmd::CreateGhCandidateList => lists::create_gh_candidate_list()?,
            Cmd::CreateGhAppList => lists::create_gh_app_list()?,
            Cmd::CreateGhCandidateListFromCache => lists::create_gh_candidate_list_from_cache()?,
            Cmd::CreateGhAppListFromCache => lists::create_gh_app_list_from_cache()?,

            // Experiment prep
            Cmd::DefineEx(ex, tc1, tc2, mode, crates) => {
                ex::define(ex::ExOpts {
                    name: ex.0,
                    toolchains: vec![toolchain::parse_toolchain(&tc1.0)?,
                                     toolchain::parse_toolchain(&tc2.0)?],
                    mode: mode,
                    crates: crates
                })?;
            }
            Cmd::PrepareEx(ex) => {
                cmds.extend(vec![Cmd::PrepareExShared(ex.clone()),
                                 Cmd::PrepareExLocal(ex)]);
            }
            Cmd::CopyEx(ex1, ex2) => ex::copy(&ex1.0, &ex2.0)?,
            Cmd::DeleteEx(ex) => ex::delete(&ex.0)?,

            // Shared emperiment prep
            Cmd::PrepareExShared(ex) => {
                cmds.extend(vec![Cmd::FetchGhMirrors(ex.clone()),
                                 Cmd::CaptureShas(ex.clone()),
                                 Cmd::DownloadCrates(ex.clone()),
                                 Cmd::FrobCargoTomls(ex.clone()),
                                 Cmd::CaptureLockfiles(ex, Tc::from_str("stable")?)]);
            }

            cmd => panic!("unimplemented cmd {:?}", cmd),
        }

        Ok((st, cmds))
    }
}

// Boilerplate conversions on the model. Ideally all this would be generated.
pub mod conv {
    use super::*;
    use errors::*;
    use clap::{App, SubCommand, Arg, ArgMatches};

    pub fn clap_cmds() -> Vec<App<'static, 'static>> {

        // Types of arguments
        let ex = opt("ex", "default");
        let ex1 = req("ex-1");
        let ex2 = req("ex-2");
        let tc = req("tc");
        let tc1 = req("tc-1");
        let tc2 = req("tc-2");
        let mode = Arg::with_name("mode")
            .required(false)
            .long("mode")
            .default_value(ExMode::BuildAndTest.to_str())
            .possible_values(&[ExMode::BuildAndTest.to_str(),
                               ExMode::BuildOnly.to_str(),
                               ExMode::CheckOnly.to_str(),
                               ExMode::UnstableFeatures.to_str()]);
        let crate_select = Arg::with_name("crate-select")
            .required(false)
            .long("crate-select")
            .default_value(ExCrateSelect::Demo.to_str())
            .possible_values(&[ExCrateSelect::Demo.to_str(),
                               ExCrateSelect::Full.to_str()]);

        fn opt(n: &'static str, def: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n)
                .required(false)
                .long(n)
                .default_value(def)
        }

        fn req(n: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n).required(true)
        }

        vec![
            // Local prep
            SubCommand::with_name("prepare-local")
                .about("acquire toolchains, build containers, build crate lists"),
            SubCommand::with_name("prepare-toolchain")
                .about("install or update a toolchain")
                .arg(tc.clone()),
            SubCommand::with_name("build-container")
                .about("build docker container needed by experiments"),

            // List creation
            SubCommand::with_name("create-lists")
                .about("create all the lists of crates"),
            SubCommand::with_name("create-lists-full")
                .about("create all the lists of crates"),
            SubCommand::with_name("create-recent-list")
                .about("create the list of most recent crate versions"),
            SubCommand::with_name("create-second-list")
                .about("create the list of of second-most-recent crate versions"),
            SubCommand::with_name("create-hot-list")
                .about("create the list of popular crates"),
            SubCommand::with_name("create-gh-candidate-list")
                .about("crate the list of all GitHub Rust repos"),
            SubCommand::with_name("create-gh-app-list")
                .about("create the list of GitHub Rust applications"),
            SubCommand::with_name("create-gh-candidate-list-from-cache")
                .about("crate the list of all GitHub Rust repos from cache"),
            SubCommand::with_name("create-gh-app-list-from-cache")
                .about("create the list of GitHub Rust applications from cache"),

            // Master experiment prep
            SubCommand::with_name("define-ex")
                .about("define an experiment")
                .arg(ex.clone()).arg(tc1.clone()).arg(tc2.clone())
                .arg(mode.clone()).arg(crate_select.clone()),
            SubCommand::with_name("prepare-ex")
                .about("prepare shared and local data for experiment")
                .arg(ex.clone()),
            SubCommand::with_name("copy-ex")
                .about("copy all data from one experiment to another")
                .arg(ex1.clone()).arg(ex2),
            SubCommand::with_name("delete-ex")
                .about("delete shared data for experiment")
                .arg(ex.clone()),

            // Global experiment prep
            SubCommand::with_name("prepare-ex-shared")
                .about("prepare shared data for experiment")
                .arg(ex.clone()),
        ]
    }

    pub fn args_to_cmd(m: &ArgMatches) -> Result<Cmd> {

        fn ex(m: &ArgMatches) -> Result<Ex> {
            Ex::from_str(m.value_of("ex").expect(""))
        }

        fn ex1(m: &ArgMatches) -> Result<Ex> {
            Ex::from_str(m.value_of("ex-1").expect(""))
        }

        fn ex2(m: &ArgMatches) -> Result<Ex> {
            Ex::from_str(m.value_of("ex-2").expect(""))
        }

        fn tc(m: &ArgMatches) -> Result<Tc> {
            Tc::from_str(m.value_of("tc").expect(""))
        }

        fn tc1(m: &ArgMatches) -> Result<Tc> {
            Tc::from_str(m.value_of("tc-1").expect(""))
        }

        fn tc2(m: &ArgMatches) -> Result<Tc> {
            Tc::from_str(m.value_of("tc-1").expect(""))
        }

        fn mode(m: &ArgMatches) -> Result<ExMode> {
            ExMode::from_str(m.value_of("mode").expect(""))
        }

        fn crate_select(m: &ArgMatches) -> Result<ExCrateSelect> {
            ExCrateSelect::from_str(m.value_of("crate-select").expect(""))
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
            ("create-gh-candidate-list", _) => Cmd::CreateGhCandidateList,
            ("create-gh-app-list", _) => Cmd::CreateGhAppList,
            ("create-gh-candidate-list-from-cache", _) => Cmd::CreateGhCandidateListFromCache,
            ("create-gh-app-list-from-cache", _) => Cmd::CreateGhAppListFromCache,

            // Master experiment prep
            ("define-ex", Some(m)) => Cmd::DefineEx(ex(m)?, tc1(m)?, tc2(m)?,
                                                    mode(m)?, crate_select(m)?),
            ("prepare-ex", Some(m)) => Cmd::PrepareEx(ex(m)?),
            ("copy-ex", Some(m)) => Cmd::CopyEx(ex1(m)?, ex2(m)?),
            ("delete-ex", Some(m)) => Cmd::DeleteEx(ex(m)?),

            // Global experiment prep
            ("prepare-ex-shared", Some(m)) => Cmd::PrepareExShared(ex(m)?),

            (s, _) => panic!("unimplemented args_to_cmd {}", s),
        })
    }

    impl ExMode {
        pub fn from_str(s: &str) -> Result<ExMode> {
            Ok(match s {
                "build-and-test" => ExMode::BuildAndTest,
                "build-only" => ExMode::BuildOnly,
                "check-only" => ExMode::CheckOnly,
                "unstable-features" => ExMode::UnstableFeatures,
                s => bail!("invalid ex-mode: {}", s),
            })
        }

        pub fn to_str(&self) -> &'static str {
            match *self {
                ExMode::BuildAndTest => "build-and-test",
                ExMode::BuildOnly => "build-only",
                ExMode::CheckOnly => "check-only",
                ExMode::UnstableFeatures => "unstable-features",
            }
        }
    }

    impl ExCrateSelect {
        pub fn from_str(s: &str) -> Result<ExCrateSelect> {
            Ok(match s {
                "full" => ExCrateSelect::Full,
                "demo" => ExCrateSelect::Demo,
                s => bail!("invalid crate-select: {}", s),
            })
        }

        pub fn to_str(&self) -> &'static str {
            match *self {
                ExCrateSelect::Full => "full",
                ExCrateSelect::Demo => "demo",
            }
        }
    }

    impl Ex {
        pub fn from_str(ex: &str) -> Result<Ex> {
            Ok(Ex(ex.to_string()))
        }
    }

    impl Tc {
        pub fn from_str(tc: &str) -> Result<Tc> {
            use toolchain;
            let _ = toolchain::parse_toolchain(tc)?;
            Ok(Tc(tc.to_string()))
        }
    }
}

pub mod state {
    use super::slowio::{FreeDir, Blobject};

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
                        gh_apps:  Blobject,
                    }
                },
                ex: ExData {
                    config: Blobject,
                }
            }
        }
    }
}

pub mod driver {
    use errors::*;

    pub trait Process<S> {
        fn process(self, s: S) -> Result<(S, Vec<Self>)> where Self: Sized;
    }

    pub fn run<S, C>(mut state: S, cmd: C) -> Result<S>
        where C: Process<S>
    {
        let mut cmds = vec!(cmd);
        loop {
            if let Some(cmd) = cmds.pop() {
                let (state_, new_cmds) = cmd.process(state)?;
                state = state_;

                // Each command execution returns a list of new commands
                // to execute, in order, before considering the original
                // complete.
                cmds.extend(new_cmds.into_iter().rev());
            } else {
                break;
            }
        }

        Ok(state)
    }
}

pub mod slowio {
    #[derive(Default)]
    pub struct FreeDir;
    #[derive(Default)]
    pub struct Blobject;
}
