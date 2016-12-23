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

## Basic local use

These commands will run cargobomb, in local configuration, on the demo
crate set. This is safe to run unsanboxed because the set of crates
tested is limited to the 'demo' set. This requires the user have
access to the docker daemon.

Today cargobomb expects to be run out of its source directory, and all
of its output is into the `./work` directory, where it maintains its
own rustup installation, crate mirrors, etc.

```
cargo run -- prepare-local
cargo run -- define-ex stable beta
cargo run -- prepare-ex
cargo run -- run
cargo run -- gen-report
```

This will output a report to `./work/ex/default/index.html`.

Here's what each of these steps does:

* `prepare-local` - sets up the stable toolchain for internal use,
  builds the docker container, builds lists of crates. This needs to
  be rerun periodically, but not between every experiment.

* `define-ex` - defines a new experiment, by default named 'default',
  performing a build-test experiment on the 'demo' set of crates.

* `prepare-ex` - fetches repos from github and captures thier commit
  shas, downloads all crates, hacks up Cargo.toml files, captures
  lockfiles, fetches all dependencies, and prepares toolchains.

* `run` - runs tests on crates in the experiment, against both
  toolchains

* `gen-report` - summarize the experiment results to
  work/ex/default/index.html

## License

MIT / Apache 2.0
