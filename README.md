# Crater

[![Build Status](https://travis-ci.org/rust-lang-nursery/crater.svg?branch=master)](https://travis-ci.org/rust-lang-nursery/crater)

Crater is a laboratory for running experiments across a large body
of Rust source code. Its primary purpose is to detect regressions in
the Rust compiler, and it does this by building large numbers of
crates, running their test suites, and comparing the results between
two versions of Rust.

It can operate completely locally, with only a dependency on docker,
or it can run distributed on AWS. It should work on Windows.

Some of the goals of Crater:

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

Crater is a successor to https://github.com/brson/taskcluster-crater.
It was subsequently named cargobomb before resuming the Crater name, so for now the code still refers to cargobomb in many places (Being addressed in #134).

__Warning: do not run Crater in an unsandboxed environment.__
___Crater executes malicious code that will destroy what you love.___

## Documentation

**User documentation:**

* [Local/CLI usage](docs/cli-usage.md)
* [Crater report triage procedure](docs/report-triage.md)
* [Legacy operational workflow](docs/legacy-workflow.md)

## License

MIT / Apache 2.0
