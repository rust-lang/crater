# next

- add frobbed toml
- create lockfiles
- run tests
- fallback gh list
- split interface into "meta", "tc", and "ex"

# not next

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
- fix docker file system permissions

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

# Data model

cargobomb works with a lot of data, and wants to coordinate that between
distributed workers

- master/
  - config.json
- local/
  - cargo-home/
  - rustup-home/
  - crates.io-index/
  - gh-clones/
  - target-dirs/
  - test/
  - custom-tc/
- shared/
  - crates/
  - gh-mirrors/
  - fromls/
  - lockfiles/
  - lists/
- ex-data/
  - $ex-name/
    - config.json
    - lockfiles/
    - custom-tc/ - ?
      - $host/
    - run/
      - $run-name/
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
- report/
  - index.html, etc.
  - ex
    - $ex-name
      - info.json
      - index.html
      - report.md
      - todo
