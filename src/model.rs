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
use self::slowio::{FreeDir, Blobject};

// An experiment name
struct Ex(String);
// A toolchain name, either a rustup channel identifier,
// or a URL+branch+sha: https://github.com/rust-lang/rust+master+sha
struct Tc(String, String, String);

enum Command {
    /* Basic synchronous commands */

    // List creation
    CreateLists { full: bool },
    CreateRecentList,
    CreateHotList,
    CreateGhCandidateList,
    CreateGhAppList,
    CreateGhCandidateListFromCache,
    CreateGhAppListFromCache,

    // Experiment prep
    DefineEx(Ex, Tc),
    PrepareEx(Ex),

    // Global experiment prep
    PrepareExShared(Ex),
    FetchGhMirrors(Ex) ,
    CaptureShas(Ex),
    DownloadCrates(Ex),
    FrobCargoTomls(Ex),
    CaptureLockfiles { ex: Ex, tc: Tc, all: bool },

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

struct GlobalState {
    master: MasterState,
    local: LocalState,
    shared: SharedState,
    ex: ExData,
}

struct MasterState;

struct LocalState {
    cargo_home: FreeDir,
    rustup_home: FreeDir,
    crates_io_index_mirror: FreeDir,
    gh_clones: FreeDir,
    target_dirs: FreeDir,
    test_dir: FreeDir,
}

struct SharedState {
    crates: FreeDir,
    gh_mirrors: FreeDir,
    lists: Lists,
}

struct Lists {
    recent: Blobject,
    second: Blobject,
    hot: Blobject,
    gh_repos: Blobject,
    gh_apps: Blobject,
}

struct ExData {
    config: Blobject,
}

mod rek {
    use errors::*;

    pub trait Process<S> {
        fn process(&self, s: S) -> Result<(S, Vec<Self>)>;
    }

    pub fn run<St, Cmd>(mut state: St, cmd: Cmd) -> Result<St>
        where Cmd: Process
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
