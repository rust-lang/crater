# next

# not next

- add single crate mode
- sort out model boilerplate
- delete cargo lockfile after build
- run `rustc -Vv` before every test
- show disk usage
- types of queries
  - Which crates depend on this crate?
  - Which crates call this function?
  - Which functions depend on this function?
  - Which crates depend on this functions?
  - Which features are used the most, by which crates?
  - Transitive unsafe usage?
  - Run regex on every crate
    - how many crate use unsafe blocks
    - unsafe blocks not for ffi
    - how many crates use x feature
    - how many crates?
    - where are lines in crates
  - Run regex on every item body via syn
  - Who is using PhantomData and why?
- mass cross-crate refactorings
- rust type system query language
- select only most recent crates.io crates
- api usage counts across ecosystem
- create toolset for doing analysis on cargobomb
- analyze unsafe rust transitively
- markdown logging
- clean ex target directory
- clean ex target directory during prepare-ex-local
- automatic bug report generation
- report tooltips for last line of output
- put time stamps in logs
- capture regressed crates to new crate 'watch' list
- set up docker init process correctly https://github.com/rust-lang/rust/pull/38340/files
- add loading progress indicator
- information to add to report
  - toolchain versions (rustc and cargo)
  - total crates tested
  - filter results by crates.io vs gh
  - job timings
  - #completed vs unknown per toolchain
  - toolchain target
  - link to lockfile
  - link to crate
  - link to froml
- generate lockfiles in parallel
- add a blacklist
- update lockfiles for repos with outdated metadata sections?
- investigate problems with toml frobbing
- test that docker works before running tests
- record durations tests take to run
- record and report on remaining work to go
- move lockfiles to non-ex data area
- make generate-lockfiles skip existing lockfiles unless --all
- fix single-threaded logging redirecting
- load crates into experiment
- single define-experiment step
- support local toolchains
- log to experiment directory
- write files atomically
- unstable feature analysis
- load crates into experiment as a discrete step
- build toolchains
- incrementalize discovery
- load gh-apps from fallback
- allow prepare-crates to update repos
- resident memory tests, compiler and tests
- instruction count tests, compiler and tests
- cache miss tests, compiler and tests
- broken blacklist
- run the rust test suite
- --cap-lints=allow
- reliable checkpoints
- aggregate benchmarking
- crate-platform compatibility matrix
- test --debug and --release
- test debug + optimizations
- use cargo-vendor for caching libraries
- associate crates with github repos
- analyze contributors to crates
- what kind of analysis can you do with
  - full source of all crate revisions
  - full dependency graph of all revisions
  - full source history of all crates
  - full source history of all rust project on github
  - compile and run sandboxed rust on all platforms
- declared dependency tester.
  for a single crate, for each of its deps, modify the lockfile while running
  that crate's tests, looking for combinations that don't pass
- crusader testing
- check for working docker before running tests
- build docker image from cargobomb

# Apps I didn't find through search :-/

https://github.com/kaj/chord3
https://github.com/dzamlo/treeify
https://github.com/azdle/virgil
https://github.com/farseerfc/ydcv-rs

# Data size

$ du work -hs
69G     work
$ du work/ex/default/ -hs
295M    work/ex/default/
$ du work/local/target-dirs/default/ -hs
48G     work/local/target-dirs/default/
$ du work/shared/crates/ -hs
7.7G    work/shared/crates/


# Building docker container

```
docker build -t cargobomb docker
```

# Data model

- master/ - 
  - state.json
- local/ - mutable state local to a machine
  - cargo-home/
  - rustup-home/
  - crates.io-index/
  - gh-clones/
  - target-dirs/
  - test/
  - custom-tc/
- shared/ - updated uniquely and shared immutably
  - crates/
  - gh-mirrors/
  - fromls/
  - lockfiles/
  - lists/
    - recent.txt
    - second.txt
    - hot.txt
    - gh-repos.txt
    - gh-apps.txt
- ex/
  - $ex/
    - config.json
    - crates.txt
    - lockfiles/
    - custom-tc/
      - $host/
    - res/
      - $tc/
        - c/
	  - $crate-id
            - result.txt
            - log.txt
        - g/
          - $gh-org/
            - $gh-name/
              - $sha/
                - result.txt
                - log.txt

# Local workflow

- prepare-local
  - prepare-toolchain stable
  - build-container
  - create-lists
    - create-recent-list
    - create-second-list
    - create-hot-list
    - create-gh-candidate-list-from-cache
    - create-gh-app-list-from-cache
- define-ex
- prepare-ex
  - prepare-ex-shared
    - fetch-gh-mirrors
    - capture-shas
    - download-crates
    - frob-tomls
    - capture-lockfiles
  - prepare-ex-custom-toolchains (todo)
  - prepare-ex-local
    - delete-all-target-dirs
    - delete-all-results
    - fetch-deps
    - prepare-all-toolchains-for-ex
- run
- gen-report
