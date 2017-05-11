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

## Operational workflow

Cargobomb is really primitive right now and leaves a lot of the
workflow up to the operator. Here's an idealized outline of how I use
it to analyze a nightly, upload reports to S3, and triage them.

- Full operation requires something like 600 GB disk space. I
  allocate a 1 TB partition and check out cargobomb there.
- I run in a screen session with 5 windows: "master", "tc1",
  "tc2", "upload"
- Create a directory to hold reports called `cargobomb-reports`. The
  "upload" screen is in this directory, all others are in `cargobomb`.
- In "master" run `cargo run -- prepare-local` once a week or so,
  between runs
- In "master" run `cargo run -- define-ex --ex nightly-2017-04-24`.
  The experiment name is named after the channel being compared to stable,
  coupled with the current date. This experiment name will correspond
  to the directory name containing the final report.
- In "master" run `cargo run -- prepare-ex --ex nightly-2017-04-24`
- In "tc1" run `cargo run -- run-tc --ex nightly-2017-04-24 stable`
- At the same time, in "tc2" run `cargo run -- run-tc --ex nightly-2017-04-24 nightly`
- That will take about 4 days. The runs can be stopped and restarted
  at any time.
- In "master" run `cargo run -- gen-report --ex nightly-2017-04-24`
- In "upload" run `rm * -r` to delete existing reports (they are
  already uploaded to S3 and will just slow down the next sync
  operation)
- In "upload" run `cp ../cargobomb/work/ex/nightly-2017-04-24/ . -r`
- In "upload" run `s3cmd -P sync ./ s3://cargobomb-reports/`
- In "master" run `cargo run -- delete-all-target-dirs --ex nightly-2017-04-24`
  to free up disk space

Now the report is uploaded to http://cargobomb-reports.s3-website-us-west-1.amazonaws.com/nightly-2017-04-24/index.html

To triage the reports I use another sandboxed Rust environment to
verify the regressions before filing them. Make sure the current
nightly/beta/stable toolchains are installed.

And for each "regressed" crate do the following:

- If this crate and revision is on the [blacklist.md], skip it.
- If the regression was actually in a _dependency_, go find _that_
  in the regression list, and deal with it first.
- Find the git repo. If I can't find it (rare) I just skip the crate.
- Check out the git repo
- If the repo has version tags, check out the corresponding version,
  otherwise use master (if master fails to reproduce I will poke around
  the commit history a bit to see if I can pull out a failing revision)
- Run `cargo +stable test` to verify that stable works.
  - If stable does not work I will run it some more to see if it's a flaky
    test, and add it to the blacklist.
  - I will run `cargo +PREVIOUS_RELEASE test` and see if that fails too,
    and if so move on.
- Run `cargo +beta test` to verify that it fails. Note that this is checking
  'beta' even if cargobomb was against 'nightly'. If that succeeds then
  I move on to `cargo +nightly test`.
- Assuming that fails, I open an issue using a [standard
  format](https://github.com/rust-lang/rust/issues/41803) filled with
  enough info to repro.
- Ping the crate author to alert them.

For crates exhibiting identical regressions, I add a comment to the
original issue mentioning the crate name and version and pinging its
author.

For crates exhibiting issues that I _know_ have been filed and
resolved, I usually decline to file them, unless there is an epidemic
of related failures that I feel need to be escalated.

When in doubt file an issue. It's best to force the Rust devs to
acknowledge the regression.

## License

MIT / Apache 2.0
