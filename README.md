# Crater [![Build Status](https://travis-ci.org/rust-lang-nursery/crater.svg?branch=master)](https://travis-ci.org/rust-lang-nursery/crater)

Crater is a tool to run experiments across parts of the Rust ecosystem. Its
primary purpose is to detect regressions in the Rust compiler, and it does this
by building large number of crates, running their test suites and comparing the
results between two versions of the Rust compiler.

It can operate locally (with Docker as the only dependency) or distributed on
the cloud. It only works on Linux at the moment, and it's licensed under both
the MIT and Apache 2.0 licenses.

The current features of Crater are:

* Discover Rust codebases on crates.io and GitHub
* Execute experiments on custom Rust toolchains
* Run `cargo build` and `cargo test` over all the discovered codebases
* Build and test without dependency updates or network access
* Run arbitrary tests over all the discovered codebases
* Generate HTML reports with results and logs
* Isolate tests in Docker containers

Crater is a successor to
[taskcluster-crater](https://github.com/brson/taskcluster-crater). It was
subsequently named cargobomb before resuming the Crater name.

:warning: **DO NOT RUN CRATER IN AN UNSANDBOXED ENVIRONMENT** :warning:  
Crater executes malicious code that will destroy what you love.

## Documentation

Want to contribute to Crater? Check out [the contribution
guide](CONTRIBUTING.md).

**User documentation:**

* [Local/CLI usage](docs/cli-usage.md)
* [GitHub bot usage](docs/bot-usage.md)
* [Crater report triage procedure](docs/report-triage.md)

**Operations documentation:**

* [Legacy operational workflow](docs/legacy-workflow.md)
* [Setting up a new Crater agent machine](docs/agent-machine-setup.md)

**Technical documentation:**

* [Agent HTTP API specification](docs/agent-http-api.md)
* [Build environment](docs/build-environment.md)
