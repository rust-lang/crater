use crate::common::CommandCraterExt;
use assert_cmd::prelude::*;
use difference::Changeset;
use rand::distributions::{Alphanumeric, DistString};
use serde_json::{self, Value};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

trait CommandMinicraterExt {
    fn minicrater_exec(&mut self);
}

impl CommandMinicraterExt for Command {
    fn minicrater_exec(&mut self) {
        if env::var_os("MINICRATER_SHOW_OUTPUT").is_some() {
            assert!(self.status().unwrap().success());
        } else {
            self.assert().success();
        }
    }
}

pub(super) struct MinicraterRun {
    pub(super) ex: &'static str,
    pub(super) crate_select: &'static str,
    pub(super) multithread: bool,
    pub(super) ignore_blacklist: bool,
    pub(super) mode: &'static str,
    pub(super) toolchains: &'static [&'static str],
}

impl Default for MinicraterRun {
    fn default() -> Self {
        MinicraterRun {
            ex: "default",
            crate_select: "demo",
            multithread: false,
            ignore_blacklist: false,
            mode: "build-and-test",
            toolchains: &["stable", "beta"],
        }
    }
}

fn expand_file_names(input: &str, add: &str) -> String {
    let mut file = input.to_string();
    file.insert_str(file.rfind('.').expect("no file extension given"), add);
    file
}

trait Compare {
    fn file_names(&self) -> Vec<String>;
    fn format(&self, input: Vec<u8>) -> Vec<u8>;
    fn compare(&self, ex_dir: &Path, file_dir: &Path) -> bool {
        let file_names = self.file_names();
        let mut failed = false;
        for file in &file_names {
            let actual_file = ex_dir.join(expand_file_names(file, ".actual"));
            let expected_file = ex_dir.join(expand_file_names(file, ".expected"));
            // Load actual report
            let raw_report = ::std::fs::read(file_dir.join(file))
                .unwrap_or_else(|_| panic!("failed to read {file}"));
            // Test report format
            let actual_report = self.format(raw_report);

            // Load the expected report
            let expected_report = ::std::fs::read(&expected_file).unwrap_or_default();

            // Write the actual JSON report
            ::std::fs::write(&actual_file, &actual_report)
                .expect("failed to write copy of the json report");

            let changeset = Changeset::new(
                &String::from_utf8(expected_report)
                    .expect("invalid utf-8 in the expected report")
                    .replace("\r\n", "\n"),
                &String::from_utf8(actual_report).expect("invalid utf-8 in the actual report"),
                "\n",
            );
            if changeset.distance != 0 {
                eprintln!(
                    "Difference between expected and actual reports:\n{}",
                    changeset
                );
                eprintln!("To expect the new report in the future run:");
                eprintln!(
                    "$ cp {} {}\n",
                    actual_file.to_string_lossy(),
                    expected_file.to_string_lossy()
                );
                failed = true;
            }
        }
        failed
    }
}

enum Reports {
    Raw,
    HTMLContext,
    MarkdownContext,
}

impl Compare for Reports {
    fn file_names(&self) -> Vec<String> {
        match *self {
            Self::Raw => vec!["results.json".into()],
            Self::HTMLContext => vec![
                "index.html.context.json".into(),
                "downloads.html.context.json".into(),
                "full.html.context.json".into(),
            ],
            Self::MarkdownContext => vec!["markdown.md.context.json".into()],
        }
    }

    fn format(&self, input: Vec<u8>) -> Vec<u8> {
        let parsed_report = match *self {
            Self::HTMLContext | Self::MarkdownContext => {
                if let Value::Object(mut map) =
                    serde_json::from_slice(&input).expect("invalid json report")
                {
                    // drop experiment field as it contains non deterministic values
                    map.remove("ex");
                    Value::Object(map)
                } else {
                    panic!("invalid json report");
                }
            }
            Self::Raw => serde_json::from_slice(&input).expect("invalid json report"),
        };
        let mut actual_report = serde_json::to_vec_pretty(&parsed_report).unwrap();
        actual_report.push(b'\n');

        actual_report
    }
}

impl MinicraterRun {
    pub(super) fn execute(&self) {
        let ex_dir = PathBuf::from("tests").join("minicrater").join(self.ex);
        let config_file = ex_dir.join("config.toml");

        let threads_count = if self.multithread {
            std::thread::available_parallelism().map_or(1, |r| r.get())
        } else {
            1
        };

        let report_dir = tempfile::tempdir().expect("failed to create report dir");
        let ex_arg = format!(
            "--ex=minicrater-{}-{}",
            self.ex,
            Alphanumeric.sample_string(&mut rand::thread_rng(), 10)
        );

        // Create local list in the temp work dir
        Command::crater()
            .args(["create-lists", "local"])
            .env("CRATER_CONFIG", &config_file)
            .minicrater_exec();

        // Define the experiment
        let mode = format!("--mode={}", self.mode);
        let crate_select = format!("--crate-select={}", self.crate_select);
        let mut define_args = vec!["define-ex", &ex_arg, &crate_select, &mode];
        define_args.extend(self.toolchains);
        if self.ignore_blacklist {
            define_args.push("--ignore-blacklist");
        }
        Command::crater()
            .args(&define_args)
            .env("CRATER_CONFIG", &config_file)
            .minicrater_exec();

        // Execute the experiment
        #[allow(clippy::needless_borrow)]
        // https://github.com/rust-lang/rust-clippy/issues/9739
        Command::crater()
            .args([
                "run-graph",
                &ex_arg,
                "--threads",
                &threads_count.to_string(),
            ])
            .args(if env::var_os("MINICRATER_FAST_WORKSPACE_INIT").is_some() {
                &["--fast-workspace-init"]
            } else {
                &[] as &[&str]
            })
            .env("CRATER_CONFIG", &config_file)
            .minicrater_exec();

        let mut failed = false;

        Command::crater()
            .args(["gen-report", &ex_arg])
            .env("CRATER_CONFIG", &config_file)
            .arg(report_dir.path())
            .arg("--output-templates")
            .minicrater_exec();

        failed |= Reports::Raw.compare(&ex_dir, report_dir.path());
        failed |= Reports::HTMLContext.compare(&ex_dir, report_dir.path());
        failed |= Reports::MarkdownContext.compare(&ex_dir, report_dir.path());

        // Delete the experiment
        Command::crater()
            .args(["delete-ex", &ex_arg])
            .env("CRATER_CONFIG", &config_file)
            .minicrater_exec();

        if failed {
            panic!("invalid report generated by Crater");
        }
    }
}

#[macro_export]
macro_rules! minicrater {
    ($( $(#[$cfg:meta])* $name:ident $opts:tt,)*) => {
        $(
            #[test]
            #[ignore]
            $(#[$cfg])*
            fn $name() {
                use $crate::minicrater::driver::MinicraterRun;
                MinicraterRun $opts.execute();
            }
        )*
    }
}
