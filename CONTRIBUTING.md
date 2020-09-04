# Contributing to Crater

Thank you for your interest in contributing to Crater! This document contains
some information to help you get started. If you have any question please join
the [`#crater` channel on the rust-lang Discord][discord]!

[discord]: https://discord.gg/MCMm5YC

## Table of Contents

[h-toc]: #table-of-contents

* [Choosing an issue to work on][h-choosing]
* [Setting up a local Crater environment][h-initial-setup]
* [Setting up a personal craterbot instance][h-craterbot-setup]
* [Submitting your pull request][h-submitting-pr]

## Choosing an issue to work on

[h-choosing]: #choosing-an-issue-to-work-on

The [issue tracker][issues] contains most of the bugs and feature requests: if
you want to work on something but you don't exactly know what, you can look for
unassigned issues in there. There are some labels you can check:

* [`E-easy`][issues-easy]: these small issues can be fixed with little effort,
  and are great to get familiar with the Crater codebase
* [`E-mentor`][issues-mentor]: these issues contain mentoring instructions in
  the comments, with some hints on how to fix the issue
* [`E-needs-help`][issues-needs-help]: the Crater developers need help to fix
  these issues, and they might take a while to be properly fixed

Please remember to comment on an issue when you start working on it, to avoid
multiple people working on the same one!

[issues]: https://github.com/rust-lang/crater/issues
[issues-easy]: https://github.com/rust-lang/crater/labels/E-easy
[issues-mentor]: https://github.com/rust-lang/crater/labels/E-mentor
[issues-needs-help]: https://github.com/rust-lang/crater/labels/E-needs-help

[Go back to the TOC][h-toc]

## Setting up a local Crater environment

[h-initial-setup]: #setting-up-a-local-crater-environment

Crater needs the latest Rust stable release to be compiled, and at the moment
it only works on Linux systems ([help with Windows support is needed][win]).
You also need to have Docker installed.

Once you cloned the repository, you can setup the local Crater environment with
the following command:

```
cargo run -- prepare-local
```

This command will setup the internal Rust toolchain used by Crater, generate
the list of crates to test and download [the Docker image] used to test the crates.

[the Docker image]: https://github.com/rust-lang/crates-build-env

You can check out the [CLI Usage][cli-usage] documentation to learn how to
interact with the Crater CLI.

[win]: https://github.com/rust-lang/crater/issues/149
[cli-usage]: docs/cli-usage.md

[Go back to the TOC][h-toc]

## Setting up a personal craterbot instance

[h-craterbot-setup]: #setting-up-a-personal-craterbot-instance

To setup a personal craterbot instance you need to have a second GitHub account
to be used as the bot, and a personal repo you can use for tests (a private one
is recommended, but a public one works just fine).

First of all you need to create your local `tokens.toml` file by copying the
example one, located at `tokens.example.toml`. Add this line at the bottom (in
the `[agents]` section):

```
"token" = "agent-1"
```

Then you need to setup the GitHub webhook that points to your local instance.
If you don't have a domain you can point to your local machine it's
recommended to use a tunnel like [ngrok][ngrok]. The Crater server will listen
on port 8000 by default.

Go to the webhooks settings of the repo you want to use for tests, and setup a
new one that points to `https://your.domain/webhooks`, accepts JSON requests
and requests the `issue_comment` events. Also you need to put a secret key of
your choice, and copy it in the `webhooks-secret` field of `tokens.toml`.

Then you need to get a personal access token for your bot account. The token
needs access to the `repo` and `read:org` scopes, and you can put it in the
`api-token` field of `tokens.toml`.

Finally you need to setup an S3-like bucket where Crater will upload the
generated reports. The `token.example.toml` file already contains credentials
for the [Minio playground][minio-play], which is free to use but frequently
resets itself. If the `crater-reports` bucket was deleted in a periodic cleanup
you can download the [Minio client][minio-client] and recreate the bucket with:

```
mc mb play/crater-reports
mc policy download play/crater-reports
```

Now you can start the server and an agent, with the following commands (execute
every one in a different terminal window):

```
cargo run -- server
cargo run -- agent http://127.0.0.1:8000 token
```

[Go back to the TOC][h-toc]

[ngrok]: https://ngrok.com/download
[minio-play]: https://play.minio.io:9000/
[minio-client]: https://www.minio.io/downloads.html#download-client

## Submitting your pull request

[h-submitting-pr]: #submitting-your-pull-request

Before submitting your pull request, you need to lint the code in the project, as otherwise the continuous integration builds will fail.

This project makes use of [`rustfmt`](https://github.com/rust-lang/rustfmt) and [`clippy`](https://github.com/rust-lang/rust-clippy) to format the code, and catch common mistakes respectively.

### Linting your code
To install rustfmt, you should follow the [quick start instructions](https://github.com/rust-lang/rustfmt#quick-start) to install it using the [`rustup`](https://rustup.rs/) tool.

To install clippy, you should follow the [usage instruction](https://github.com/rust-lang/rust-clippy#usage) to install it using the [`rustup`](https://rustup.rs/) tool.

To lint the code, run `cargo fmt` to format your code and `cargo clippy` to catch common mistakes and improve your code.

[Go back to the TOC][h-toc]
