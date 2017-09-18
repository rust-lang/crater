# cargobomb

[![Build Status](https://travis-ci.org/rust-lang-nursery/cargobomb.svg?branch=master)](https://travis-ci.org/rust-lang-nursery/cargobomb)

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

* `prepare-ex` - fetches repos from github and captures their commit
  shas, downloads all crates, hacks up Cargo.toml files, captures
  lockfiles, fetches all dependencies, and prepares toolchains.

* `run` - runs tests on crates in the experiment, against both
  toolchains

* `gen-report` - summarize the experiment results to
  work/ex/default/index.html

### Custom toolchains

Toolchains for rust PRs that have been built by by asking bors to try a PR can
be specified using `try#<SHA1 of try merge>`. You will probably want to specify
the comparison commit as `master#<SHA1 of master before try merge>`.

## Operational workflow for production cargobomb

### Getting access

There are three 'official' cargobomb machines:

 - cargobomb-test (54.177.234.51) - 1 core, 4GB RAM, for experimenting
 - cargobomb-try (54.241.86.211) - 8 core, 30GB RAM, for doing PR runs
 - cargobomb-prod (54.177.126.219) - 8 core, 30GB RAM, for doing beta runs (but can do PR runs if free)

These can only be accessed via the bastion - you `ssh` to the bastion,
then `ssh` to the cargobomb machine. The bastion has restricted access
and you will need a static IP address (if you have a long-running server
in the cloud, that's usually fine) and a public SSH key (you should add
the key to github and then link to https://github.com/yourusername.keys,
once you have access to the bastion you can manage your own keys).

With these two pieces of information in hand, ask acrichto to
add you to the bastion and all three machines and they'll let you know
the bastion IP. You can now either edit your `~/.ssh/config` on your
static IP machine to contain

```
Host rust-bastion
    # Bastion IP below
    HostName 0.0.0.0
    User bastionusername
Host cargobomb-test
    HostName 54.177.234.51
    ProxyCommand ssh -q rust-bastion nc -q0 %h 22
    User ec2-user
# [...and so on for cargobomb-try and cargobomb-prod...]
```

which will let you do `ssh cargobomb-test` etc from your static IP
machine. If you have a recent OpenSSH, you can use `ProxyJump` instead.

### General cargobomb server notes

The cargobomb servers use a terminal multiplexer (a way to keep multiple
terminals running on a server). Enter the multiplexer by logging onto a
server and running `byobu`. You'll notice a bit of text along the
bottom saying something like "0:master 1:tc1 2:tc2" - these are
the 'windows' in the terminal multiplexer. The one highlighted and with a
`*` next to it is the current window. Sending commands to the multiplexer
is achieved by pressing Ctrl+Z, and then another key.

Some useful operations:
 - Ctrl+Z d - detach from the multiplexer (or you can just close your
   terminal)
 - Ctrl+Z 0 - switch to window 0 (or any other number)
 - Ctrl+Z PageUp - scroll upwards on the terminal. This will enter a
   sort of 'scrolling mode', so you can use PageUp and PageDown freely
   (to the limit of terminal scrollback). To return to normal terminal
   mode, hit Ctrl+C - be sure to only press it once, or you risk
   returning to normal mode and then killing the process running in the
   current terminal!
 - Ctrl+Z c - create a new window, useful if you accidentally closed one
 - Ctrl+Z , - rename a window, useful after recreating an accidentally
   closed window (hit enter to accept new name)

### Cargobomb triage process

On your day for cargobomb triage, open the
[sheet](https://docs.google.com/spreadsheets/d/1VPi_7ErvvX76fa3VqvQ3YnQmDk3bS7fYnkzvApIWkKo/edit#gid=0).
Click the top left cell and make sure every PR on that list has an
entry on the sheet and make sure every row on the sheet without
'Complete' or 'Failed' is listed on the GitHub search. You may need
to update PR tags or add rows to the sheet as appropriate.

Next, you should follow the steps below for eachrequested run on
the sheet that does not have a status of 'Complete' or 'Failed'.

 - Pending
   - Is try or prod available? (prioritise beta runs to go on prod, no
     matter how far down the pending list they are) If not, go to next run.
   - Log onto appropriate box and connect to multiplexer.
   - Double check each multiplexer window to make sure nothing is
     running.
   - Switch to the `master` multiplexer window.
   - Run `docker ps` to make sure no containers are running.
   - Run `df -h /home/ec2-user/cargobomb/work`, disk usage should be
     <250GB of the 1TB disk (a full run may consume 600GB)
     - If disk usage is greater, there are probably target directories
       left over from a previous run. Run `du -sh work/local/target-dirs/*`,
       find the culprit (likely a directory with >100GB).
     - The directory name is the name of an experiment, e.g. MY_EX, so run
       `cargo run --release -- delete-all-target-dirs --ex MY_EX`.
   - Run `docker ps -aq | xargs --no-run-if-empty docker rm` to clean up all terminated
     Docker containers.
   - Run `git stash && git pull && git stash pop` to get the latest cargobomb changes.
     If this fails, it means there were local changes that conflict with upstream
     changes. Ping aidanhs and tomprince on IRC.
   - Run `cargo run --release -- prepare-local`. This may take between 5s and 5min, depending
     on what needs doing.
   - Log `EX_NAME`, `EX_START` and `EX_END` in the spreadsheet, where:
     - If doing a run for PR 12345, `EX_NAME` is `pr-12345`, `EX_END` is
       `try#deadbeef2...` (`deadbeef2` is in the bors comment "Trying commit
       `abcdef` with merge `deadbeef2`" - click through and copy from the URL to get the full
       commitish) and `EX_START` is `master#deadbeef1...` (`deadbeef1` is on the page you clicked
       through to get `deadbeef2...`, just below the commit message, the left hand commit of
       "2 parents `deadbeef1` and `bcdef1`" - click through and copy from the URL to get the
       full commitish, make sure the commit is an auto merge from bors). Just to emphasise,
       the second commitish you copied goes in `EX_START`.
     - If doing a beta run, `EX_NAME` is `stable-STABLE_VERSION-beta-BETA_VERSION`, `EX_START` is
       `LAST_STABLE` and `EX_END` is `BETA_DATE`. `STABLE_VERSION` is the version number from
       `curl -sSL static.rust-lang.org/dist/channel-rust-stable.toml | grep -A1 -F '[pkg.rust]'`,
       `BETA_VERSION` is the version number from
       `curl -sSL static.rust-lang.org/dist/channel-rust-beta.toml | grep -A1 -F '[pkg.rust]'`
       and `BETA_DATE` is the date from
       `curl -sSL static.rust-lang.org/dist/channel-rust-beta.toml | grep '^date ='` (it is *not*
       necessarily the same date as retrieved in the `BETA_VERSION` command).
   - Run `cargo run --release -- define-ex --ex EX_NAME EX_START EX_END --crate-select=full`.
     This will complete in a few seconds.
   - Run `cargo run --release -- prepare-ex --ex EX_NAME`.
   - Change status to 'Preparing'.
   - Update either the PR or the person requesting the run to let them know the run has started.
   - Go to next run.
 - Preparing
   - Log onto appropriate box and connect to multiplexer.
   - Switch to the `master` multiplexer window.
   - If preparation is ongoing, go to next run.
   - If preparation failed, fix it. Known errors:
     - "missing sha for ..." - remove the referenced repository from `gh-apps.txt`
       and `gh-candidates.txt` (may be present in one or both). Make the same
       change locally and make a PR against cargobomb. Use
       `cargo run --release -- delete-all-target-dirs --ex EX_NAME` and
       `cargo run --release -- delete-ex --ex EX_NAME`, then jump to start of 'Pending'.
   - Switch to the `tc1` multiplexer window.
   - Run `cargo run --release -- run-tc --ex EX_NAME EX_START`.
   - Switch to the `tc2` multiplexer window.
   - Run `cargo run --release -- run-tc --ex EX_NAME EX_END`.
   - Go to next run.
 - Running
   - Log onto appropriate box and connect to multiplexer.
   - Switch to the `master` multiplexer window.
   - Run `docker ps`. If any container has been running for more than 30min (may
     need to follow these steps more than once):
     - Take solace in us someday fixing this for good with docker limits.
       TODO: actually fix. Seems to only be a problem on prod with pleingres,
       our existing limits should catch it.
     - Run `docker top CONTAINER_ID`.
     - If there's no mention of pleingres, raise an issue with the output of
       the previous `docker top` command.
     - The process at the bottom of the list is the lowest in the process tree,
       and should have a value in the `TIME` column of >30min. Find the value in
       the `PID` column and run `kill PID`.
     - Wait a few seconds, then check the container has now exited.
   - If the run is ongoing in either the `tc1` or `tc2` multiplexer
     windows, go to next run.
   - Switch to the `master` multiplexer window.
   - Run `du -sh work/ex/EX_NAME`, output should be <2GB. If not:
     - Run `find work/ex/EX_NAME -type f -size +100M | xargs du -sh`,
       there will likely only be a couple of files listed and they
       should be in the `res` directory (TODO: blacklist pleingres
       as the main culprit here once it's possible, and update
       these instructions to suggest adding things to the blacklist).
     - For each file found, run `truncate --size='<100M' FILE`.
     - Check `du -sh work/ex/EX_NAME` is now an appropriate size.
   - Run `cargo run --release -- publish-report --ex EX_NAME s3://cargobomb-reports/EX_NAME`.
   - Change status to 'Uploading'.
   - (optional but much appreciated: come back to this run in 30mins
     as the upload will be complete)
   - Go to next run.
 - Uploading
   - Switch to the `master` multiplexer window.
   - If the upload is ongoing, go to the next run.
   - If the upload failed, fix it. Known errors:
     - `<Error><Code>InternalError</Code><Message>...` - probably an s3 failure, try running
       upload again.
   - Run `cargo run --release -- delete-all-target-dirs --ex EX_NAME`. This will take ~2min.
   - Change status to 'Complete' and add the results link,
     `http://cargobomb-reports.s3.amazonaws.com/EX_NAME/index.html`.
   - Update either the PR or the person requesting the beta run. Template is:
     ```
     Cargobomb results: <url>. 'Blacklisted' crates (spurious failures etc) can be found
     [here](https://github.com/rust-lang-nursery/cargobomb/blob/master/blacklist.md).
     If you see any spurious failures not on the list, please make a PR against that file.
     ```
   - Give yourself a pat on the back! Good job!
   - Go to next run.

(The runs can be stopped and restarted at any time. - really? How? asks aidanhs)

If a beta run has completed, regressions need reporting (PR runs are left to the
people involved in the PR). To report regressions you'll need to
navigate to the results page, wait for a bit (<30s) for the results to load (the
buttons will be populated with numbers) and then click 'regressed'. The triage
process (e.g. checking the cause of a regression) is 'crowd-sourced', we just
report the issues (for now).

You can follow whatever process you like for working through regressions,
but a suggestion workflow is described below, per regression:

 - Open the regression log (i.e. 'toolchain 2').
 - If the regression is on the [blacklist](blacklist.md), skip it.
 - If the breakage is 'obviously deliberate', e.g. a lint changing to deny by
   default, find the original PR and double check it went through a cargobomb
   run. Skip reporting if so.
 - If the regression is in a dependency, it will have probably caused multiple
   regressions so make sure to deal with the dependency first and then ignore
   any duplicates.
 - If this is not a .1 beta (i.e. it's a second beta run), search for the
   regression already being reported. If it was closed as "wanted regression"
   skip reporting, if it was closed as "fixed" then reopen with a link to the
   log.
 - Report the regression per the template below:

This template varies depending on crate source (crates.io or a git repo):
```
[CRATENAME-1.0.1](https://crates.io/crates/cratename) regressed from stable to beta - http://cargobomb-reports.../log.txt, cc @AUTHOR
[AUTHOR/REPO#COMMITISH](https://github.com/author/repo/tree/COMMITISH) regressed from stable to beta - http://cargobomb-reports.../log.txt, cc @AUTHOR
```
where AUTHOR is the github username of the crate author (may not be available
if the crate is from crates.io in rare cases). You should also paste a snippet
of the error in the issue.

When in doubt file an issue. It's best to force the Rust devs to
acknowledge the regression.

If you are interested in triaging once the issues are raised,
you can follow the rough instructions below (to be made clearer):

To triage the reports I use another sandboxed Rust environment to
verify the regressions before filing them. Make sure the current
nightly/beta/stable toolchains are installed.

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

## License

MIT / Apache 2.0
