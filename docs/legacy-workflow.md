# Legacy operational workflow for production Crater

This is the legacy workflow for production Crater. It's going to be replaced
with bot-controlled Crater, and it's meant to be used only by infra team
members.

## Getting access

There are three 'official' Crater machines:

 - cargobomb-test (54.177.234.51) - 1 core, 4GB RAM, for experimenting
 - cargobomb-try (54.241.86.211) - 8 core, 30GB RAM, for doing PR runs
 - cargobomb-prod (54.177.126.219) - 8 core, 30GB RAM, for doing beta runs (but can do PR runs if free)

These can only be accessed via the bastion - you `ssh` to the bastion,
then `ssh` to the Crater machine. The bastion has restricted access
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

## General Crater server notes

The Crater servers use a terminal multiplexer (a way to keep multiple
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

## Crater triage process

On your day for Crater triage, open the
[sheet](https://docs.google.com/spreadsheets/d/1VPi_7ErvvX76fa3VqvQ3YnQmDk3bS7fYnkzvApIWkKo/edit#gid=0).
Click the top left cell and make sure every PR on that list has an
entry on the sheet and make sure every row on the sheet without
'Complete' or 'Failed' is listed on the GitHub search. You may need
to update PR tags or add rows to the sheet as appropriate.

Next, you should follow the steps below for eachrequested run on
the sheet that does not have a status of 'Complete' or 'Failed'.

 - Pending
   - Log onto appropriate box and connect to multiplexer by running `byobu`.
   - Double check each multiplexer window to make sure nothing is running.
   - Switch to the `master` multiplexer window.
   - Run `docker ps` to make sure no containers are running.
   - Run `df -h /home/ec2-user/crater/work`, disk usage should be
     <250GB of the 1TB disk (a full run may consume 600GB)
     - If disk usage is greater, there are probably target directories
       left over from a previous run. Run `du -sh work/local/target-dirs/*`,
       find the culprit (likely a directory with >100GB).
     - The directory name is the name of an experiment, e.g. MY_EX, so run
       `cargo run --release -- delete-all-target-dirs --ex MY_EX`.
   - Run `docker ps -aq | xargs --no-run-if-empty docker rm` to clean up all terminated
     Docker containers.
   - Run `git stash && git pull && git stash pop` to get the latest Crater changes.
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
   - Run `cargo run --release -- define-ex --crate-select=full --ex EX_NAME EX_START EX_END`.
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
       change locally and make a PR against Crater. Use
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
   - If the run is ongoing in either the `tc1` or `tc2` multiplexer
     windows, go to next run.
   - Switch to the `master` multiplexer window.
   - Run `du -sh work/ex/EX_NAME`, output should be <2GB. If not:
     - Run `find work/ex/EX_NAME -type f -size +100M | xargs --no-run-if-empty du -sh`,
       there will likely only be a couple of files listed and they should be in the `res` directory.
     - Run ` find work/ex/EX_NAME -type f -size +100M | xargs truncate --size='<100M'`.
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
     > Hi X (crater requester), Y (PR reviewer)! Crater results are at: \<url>. 'Blacklisted' crates (spurious failures etc) can be found \[here\](https://github.com/rust-lang-nursery/crater/blob/master/config.toml). If you see any spurious failures not on the list, please make a PR against that file.
     >
     > (interested observers: Crater is a tool for testing the impact of changes on the crates.io ecosystem. You can find out more at the \[repo\](https://github.com/rust-lang-nursery/crater/) if you're curious)
   - Give yourself a pat on the back! Good job!
   - Go to next run.

(The runs can be stopped and restarted at any time. - really? How? asks aidanhs)
