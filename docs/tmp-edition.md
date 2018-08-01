# Quick reference for edition runs

This is just temporary with the `tmp-edition` branch -- hopefully it will be
nicer once proper support is added.

```
$ cargo run --release -- define-ex --ex edition-N nightly-1970-01-01 nightly-1970-01-01+tmprustfix --cap-lints=forbid --mode=tmprustfix --crate-select=full
$ ./craterbot.sh cargo run --release -- run-graph --ex edition-N --threads 8
$ ./craterbot.sh cargo run --release -- publish-report --ex edition-N s3://cargobomb-reports/edition-N
```

Some notes:

* During `define-ex`, the second toolchain needs to have the `+tmprustfix`
  flag, to apply the changes needed for that toolchain
* During `define-ex`, the mode needs to be `tmprustfix`, to execute the custom
  code needed for rustfix (it then executes the `build-and-test` mode)
* During `publish-report`, let's upload to the `cargobomb-reports` bucket
  instead of the newer `crater-reports`: I *think* only the `rust-bots` machine
  can upload to the newer one, and not the agent machines
