# Bot usage

Crater can be controlled in the rust-lang/rust repo thanks to the GitHub bot
[@craterbot](https://github.com/craterbot). The bot replies to every command in
the comments of issues and pull requests, if the command is in its own line and
is prefixed with the bot's username.

For example, to check if the bot is alive you can write this comment:

```
@craterbot ping
```

And the bot will reply to you.

## Creating experiments

You can create experiments with the `run` command. For example, to create a
beta run you can use:

```
@craterbot run name=foobar start=stable end=beta cap-lints=allow
```

* `name`: name of the experiment; required only if Crater [can't determine it
  automatically][auto-name]
* `start`: name of the first toolchain; can be either a rustup name or
  `branch#sha` (required)
* `end`: name of the second toolchain; can be either a rustup name or
  `branch#sha` (required)
* `mode`: the experiment mode (default: `build-and-test`)
* `crates`: the selection of crates to use (default: `full`)
* `cap-lints`: the lints cap (default: `forbid`, which means no cap)
* `p`: the priority of the run (default: `0`)

## Editing experiments

Experiments can be edited as long as they're queued. To edit an experiment,
send a command with the options you want to change. For example, to change the
priority of the `foo` experiment you can use:

```
@craterbot name=foo p=1
```

* `name`: name of the experiment; required only if Crater [can't determine it
  automatically][auto-name]
* `start`: name of the first toolchain; can be either a rustup name or
  `branch#sha` (required)
* `end`: name of the second toolchain; can be either a rustup name or
  `branch#sha` (required)
* `mode`: the experiment mode (default: `build-and-test`)
* `crates`: the selection of crates to use (default: `full`)
* `cap-lints`: the lints cap (default: `forbid`, which means no cap)
* `p`: the priority of the run (default: `0`)

## Aborting experiments

If you don't want to run an experiment anymore, you can use the `abort`
command. For example, to abort an experiment named `foo` you can use:

```
@craterbot abort name=foo
```

* `name`: name of the experiment; required only if Crater [can't determine it
  automatically][auto-name]

## Automatic experiment names

Crater tries to predict what the name of the experiment you're working on is,
and in those cases you aren't required to explicitly provide one during
commands. At the moment, the name is predicted in these cases:

* If you already used a name in the issue/PR, that name is reused by default
  for future requests
* If you didn't use a name before and you're in a PR, `pr-NUMBER` is used as
  default (for example `pr-12345`)

[auto-name]: #automatic-experiment-names
