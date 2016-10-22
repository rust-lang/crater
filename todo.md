# next

- add frobbed toml
- create lockfiles
- run tests
- fallback gh list
- split interface into "meta", "tc", and "ex"

# not next

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

# Apps I didn't find through search :-/

https://github.com/kaj/chord3
https://github.com/dzamlo/treeify
https://github.com/azdle/virgil
https://github.com/farseerfc/ydcv-rs

# Data size

5.5 GB for the most recent crates
10M lines Rust in most recent crates
8.6 GB after fetching deps
3152 deps cached
13 GB after building up to bytestool
