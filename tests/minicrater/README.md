# minicrater

minicrater is the component of Crater's test suite that tests if the runs are
actually executed and produce the correct result. It's inspired by rustc's
compiletest.

As the name implies minicrater is simply a "mini" version of crater, running 
in a similar fashion as normal crater but on a much smaller scale to enable
testing.

## Executing minicrater

minicrater executions can take a few minutes to run, so it's ignored by default
while running `cargo test`. You can run minicrater with:

```
$ cargo test minicrater -- --ignored --test-threads 1
```

The runs' output is hidden by default, but you can show it by setting the
`MINICRATER_SHOW_OUTPUT` environment variable:

```
$ MINICRATER_SHOW_OUTPUT=1 cargo test minicrater -- --ignored --test-threads 1
```

## Adding tests to minicrater

There are two ways to add a test to minicrater:

* If your test requires different experiment configuration (like different
  flags or `config.toml` file entries) and no other minicrater run has the
  configuration you want you need to create a new minicrater run
* Otherwise you can create a new local crate and have it tested by an existing
  minicrater run (usually the full ones)

minicrater runs are defined in `tests/minicrater/mod.rs`, and each run has
additional configuration in the `tests/minicrater/<run>` directory: a
`config.toml` with the configuration file used for the run, and
`results.expected.json`, which contains the JSON output expected in the
generated report.

### Adding new local crates

minicrater doesn't test public crates available on crates.io, but "local
crates", which are dummy crates located in the `local-crates` directory at the
top of the project. To add a new local crate you need to actually create the
crate in that directory and then run:

```
$ cargo run create-lists
```

The `create-lists` command is only needed when you add or remove local crates,
not when you edit one of them.

### Adding new minicrater runs

To add a new minicrater run, you need to add a new entry to
`tests/minicrater/mod.rs` and a configuration file in
`tests/minicrater/<run>/config.toml`. Then you run minicrater - expecting the
experiment to be failing - and execute the command shown in the output to
generate the expected JSON report.
