# cargobomb - raining fiery death on Rust regressions

Cargobomb is a laboratory for running experiments across a large corpus of Rust
code. Its goals are:

- Discover Rust codebases from crates.io and GitHub
- Download all Rust code to a local disk
- Build and manage custom Rust toolchains
- Run `cargo build` and `cargo test` over all codebases
- Cache dependencies to avoid unnecessary rebuilds
- Control variables during testing. Lockfiles are shared between comparable test
  runs. Network operations are done prior to testing. All tests are performed
  with `--frozen`.
- Run arbitrary tests over all codebases
- Resume partial test runs
- Compare results between test runs
- Generate summary HTML and text reports
- Run on Linux and Windows
- Isolate tests into docker containers on Linux and Windows 2016
- Test against Linux-based cross targets under docker
- Hosted, distributed testing

Cargobomb is a successor to [crater].

__Warning: do not run cargobomb in an unsandboxed environment.__  
__Cargobomb executes arbitrary code and WILL DESTROY EVERYTHING YOU LOVE.__

_Note: cargobomb does not work yet. There is nothing to see here._

[crater]: https://github.com/brson/taskcluster-crater
