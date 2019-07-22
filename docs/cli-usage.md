# Basic local use

These commands will run Crater, in local configuration, on the demo
crate set. This should be safe to run locally because the set of crates
tested is limited to the 'demo' set which you can control to crates you 
trust. Of course, the experiments are still run inside of Docker so there 
is _some_ form of sandboxing. Of course, this means you'll need Docker
running locally.

Today Crater expects to be run out of its own source directory so you'll
want to clone it first. All of its output is put into the `./work` directory, 
where it maintains its own rustup installation, crate mirrors, a database
of experiments, etc.

To set up the local crater directory to run experiments first run the following:
```bash
cargo run -- prepare-local
```

You can then define your own experiment like so:
```bash
cargo run -- define-ex --crate-select=demo --cap-lints=forbid stable beta
```

This will create an experiment named "default", but you can give your experiment
a more meaningful name using the `--ex` option. The configuration for which crates
will be run in the experiment is definied the `config.toml` file found at the root
of the repo. In this config file you'll find the `demo-crates` section which defines
three sets of crates that combine to form the set of crates being tests. The `crates`
section is the name of crates on crates.io, the `github-repos` section is a list of
github repos, and the `local-crates` section is a list of creates located in the 
`local-crates` directory in this repo.

To actually run the experiment do the following:
```bash
cargo run -- run-graph --threads NUM_CPUS
```

Remember to pass the `--ex` option if you gave your experiment a distinct name.

To see a report of the results, run the following:

```bash 
cargo run -- gen-report work/ex/default/
```

This will output a report to `./work/ex/default/index.html`.

If you want to clean things up you can use the following commands:
```bash
# delete all the target directories
cargo run -- delete-all-target-dirs
# delete all results directories
cargo run -- delete-all-results
# remove a specific experiment
cargo run -- delete-ex 
```

Reminder: Each command except `prepare-local` optionally takes an `--ex` argument
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
