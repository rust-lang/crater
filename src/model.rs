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
#[derive(Debug)]
pub struct Ex(String);
// A toolchain name, either a rustup channel identifier,
// or a URL+branch+sha: https://github.com/rust-lang/rust+master+sha
#[derive(Debug)]
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

    // Experiment prep
    DefineEx(Ex, Tc, Tc, ExMode, ExCrateSelect),
    PrepareEx(Ex),
    CopyEx(Ex, Ex),
    DeleteEx(Ex),

    // Global experiment prep
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

use driver::Process;
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

            Cmd::DefineEx(ex, tc1, tc2, mode, crates) => {
                ex::define(ex::ExOpts {
                    name: ex.0,
                    toolchains: vec![toolchain::parse_toolchain(&tc1.0)?,
                                     toolchain::parse_toolchain(&tc2.0)?],
                    mode: mode,
                    crates: crates
                })?;
            }

            cmd => panic!("unimplemented cmd {:?}", cmd),
        }

        Ok((st, cmds))
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
