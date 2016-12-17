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
struct Ex(String);
// A toolchain name, either a rustup channel identifier,
// or a URL+branch+sha: https://github.com/rust-lang/rust+master+sha
struct Tc(String, String, String);

enum Cmd {
    /* Basic synchronous commands */

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
    DefineEx(Ex, Tc, Tc),
    PrepareEx(Ex),

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

    // Reporting
    GenReport(Ex),

    // Misc
    PrepareToolchain(Tc),
    LinkToolchain,
    Run,
    RunUnstableFeatures,
    Summarize,
    EasyTest,
    Sleep,
}

mod state {
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
        test_dir: FreeDir,
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
}

use self::state::GlobalState;

impl rek::Process<GlobalState> for Cmd {
    fn process(self, st: GlobalState) -> Result<(GlobalState, Vec<Cmd>)> {
        use lists;
        let mut cmds = Vec::new();
        match self {
            Cmd::CreateLists => {
                cmds.extend(vec![Cmd::CreateRecentList,
                                 Cmd::CreateSecondList,
                                 Cmd::CreateHotList,
                                 Cmd::CreateGhCandidateList,
                                 Cmd::CreateGhAppList]);
            }
            Cmd::CreateListsFull => {
                cmds.extend(vec![Cmd::CreateRecentList,
                                 Cmd::CreateSecondList,
                                 Cmd::CreateHotList,
                                 Cmd::CreateGhCandidateListFromCache,
                                 Cmd::CreateGhCandidateListFromCache]);
            }
            Cmd::CreateRecentList => lists::create_recent_list()?,
            Cmd::CreateSecondList => lists::create_second_list()?,
            Cmd::CreateHotList => lists::create_hot_list()?,
            Cmd::CreateGhCandidateList => lists::create_gh_candidate_list()?,
            Cmd::CreateGhAppList => lists::create_gh_app_list()?,
            Cmd::CreateGhCandidateListFromCache => lists::create_gh_candidate_list_from_cache()?,
            Cmd::CreateGhAppListFromCache => lists::create_gh_app_list_from_cache()?,
            _ => unimplemented!()
        }

        Ok((st, cmds))
    }
}

mod rek {
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

mod slowio {
    pub struct FreeDir;
    pub struct Blobject;
}
