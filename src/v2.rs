/*
- download
- copy_dir
- remove_dir_all
- dojob
- hardlink
- flock
- symlink_dir
- atomic replace
*/

struct CargobombState {
    master: UniqueDir<Master>,
    local: UniqueDir<Local>,
    shared: UniqueDir<Shared>,
    ex: UniqueDir<Ex>,
}

struct Master {
    master_state: AtomicFile,
}

struct Local {
    cargo_home: MutableDir
    rustup_home: MutableDir
    crates_io_mirror: MutableDir
    gh_clones: MutableDir
    target_dirs: MutableDir
}

struct Shared {
    crates: ImmutableDirStore
    gh_mirrors: ImmutableDirStore
    fromls: ImmutableFileStore
    lockfiles: MutableFileStore,
}

struct Ex {
    header: UniqueDir<ExHeader>,
    run: 
}

struct ExHeader {
    config: AtomicFile,
    crates: AtomicFile,
    lockfiles: ImmutableFileStore
}
