# cargobomb

Cargobomb is a laboratory for running experiments across a large body
of Rust source code. Its primary purpose is to detect regressions in
the Rust compiler, and it does this by building large numbers of
crates, running their test suites, and comparing the results between
two versions of Rust.

It can operate completely locally, with only a dependency on docker,
or it can run distributed on AWS. It should work on Windows.

Some of the goals of cargobomb:

- Discover Rust codebases from crates.io and GitHub
- Download all Rust code to a local disk
- Build and manage custom Rust toolchains
- Run `cargo build` and `cargo test` over all codebases
- Cache dependencies to avoid unnecessary rebuilds
- Lockfiles shared between runs
- Dependencies fetched ahead of time
- Building and testing is `--frozen` - no dependency updates or network access
- Run arbitrary tests over all codebases
- Resume partial test runs
- Generate summary HTML and text reports
- Run on Linux and Windows
- Isolate tests into docker containers on Linux and Windows
- Test against Linux-based cross targets under docker
- Hosted, distributed testing on AWS

Cargobomb is a successor to [crater].

__Warning: do not run cargobomb in an unsandboxed environment.__  
___Cargobomb executes malicious code that will destroy what you love.___  
_Cargobomb does not work yet. There is nothing to see here._

[crater]: https://github.com/brson/taskcluster-crater

## License

MIT / Apache 2.0
