# Basic local use

These commands will run Crater, in local configuration, on the demo
crate set. This is safe to run unsanboxed because the set of crates
tested is limited to the 'demo' set. This requires the user have
access to the docker daemon.

Today Crater expects to be run out of its source directory, and all
of its output is into the `./work` directory, where it maintains its
own rustup installation, crate mirrors, etc.

```
cargo run -- prepare-local
cargo run -- define-ex --crate-select=demo --cap-lints=forbid stable beta
cargo run -- run-graph --threads NUM_CPUS
cargo run -- gen-report work/ex/default/
```

This will output a report to `./work/ex/default/index.html`.

Delete things with
```
cargo run -- delete-all-target-dirs
cargo run -- delete-all-results
cargo run -- delete-ex
```
Each command except `prepare-local` optionally takes an `--ex` argument
to identify the experiment being referred to. If not supplied, this
defaults to `default`. Here's what each of the steps does:

* `prepare-local` - sets up the stable toolchain for internal use,
  builds the docker container, builds lists of crates. This needs to
  be rerun periodically, but not between every experiment.

* `define-ex` - defines a new experiment
  performing a build-test experiment on the 'demo' set of crates.

* `run-graph` - executes the experiment. You can control the number of parallel
  tasks executed with the `--threads` flag.

* `run` - runs tests on crates in the experiment, against both
  toolchains

* `gen-report` - summarize the experiment results to
  work/ex/default/index.html

* `delete-all-target-dirs`/`delete-all-results`/`delete-ex` - clean up
  everything relating to this experiment

## Custom toolchains

Toolchains for rust PRs that have been built by asking bors to try a PR can
be specified using `try#<SHA1 of try merge>`. You will probably want to specify
the comparison commit as `master#<SHA1 of master before try merge>`.
