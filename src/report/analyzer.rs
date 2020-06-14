use super::{Comparison, CrateResult, RawTestResults};
use crate::crates::Crate;
use crate::results::{
    FailureReason,
    TestResult::{self, BuildFail},
};
use std::collections::BTreeSet;
use std::collections::HashMap;

pub enum ReportConfig {
    Simple,
    Complete { toolchain: usize },
}

#[cfg_attr(test, derive(Debug, PartialEq))]
#[derive(Clone)]
pub enum ReportCrates {
    Plain(Vec<CrateResult>),
    Complete {
        tree: HashMap<Crate, Vec<CrateResult>>,
        results: HashMap<TestResult, Vec<CrateResult>>,
    },
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct TestResults {
    pub categories: HashMap<Comparison, ReportCrates>,
    pub info: HashMap<Comparison, u32>,
}

fn analyze_detailed(toolchain: usize, crates: Vec<CrateResult>) -> ReportCrates {
    let mut tree = HashMap::new();
    let mut results = HashMap::new();

    let mut root = Vec::new();
    for krate in crates {
        if let BuildFail(FailureReason::DependsOn(ref deps)) =
            (&krate.runs[toolchain]).as_ref().unwrap().res
        {
            for dep in deps {
                tree.entry(dep.clone())
                    .or_insert_with(Vec::new)
                    .push(krate.clone())
            }
        } else {
            root.push(krate);
        }
    }

    for krate in root {
        // record results only for root crates
        if let BuildFail(FailureReason::CompilerError(codes)) =
            krate.runs[toolchain].clone().unwrap().res
        {
            for code in codes {
                results
                    .entry(BuildFail(FailureReason::CompilerError(btreeset![code])))
                    .or_insert_with(Vec::new)
                    .push(krate.clone())
            }
        } else {
            results
                .entry(krate.runs[toolchain].as_ref().unwrap().res.clone())
                .or_insert_with(Vec::new)
                .push(krate)
        }
    }

    ReportCrates::Complete { tree, results }
}

pub fn analyze_report(test: RawTestResults) -> TestResults {
    let mut comparison = HashMap::new();
    for krate in test.crates {
        comparison
            .entry(krate.res)
            .or_insert_with(Vec::new)
            .push(krate);
    }

    let info = comparison
        .iter()
        .map(|(&key, vec)| (key, vec.len() as u32))
        .collect::<HashMap<_, _>>();

    let mut categories = HashMap::new();
    for (cat, crates) in comparison {
        if let ReportConfig::Complete { toolchain } = cat.report_config() {
            categories.insert(cat, analyze_detailed(toolchain, crates));
        } else {
            categories.insert(cat, ReportCrates::Plain(crates));
        }
    }

    TestResults { categories, info }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::crates::{Crate, RegistryCrate};
    use crate::experiments::{CapLints, Experiment, Mode, Status};
    use crate::report::{generate_report, Comparison};
    use crate::results::{DummyDB, FailureReason::*};
    use crate::toolchain::{MAIN_TOOLCHAIN, TEST_TOOLCHAIN};
    use failure::Fallible;

    #[test]
    fn test_report_analysis() -> Fallible<()> {
        macro_rules! reg {
            ($name:expr) => {
                Crate::Registry(RegistryCrate {
                    name: $name.into(),
                    version: "0.0.1".into(),
                })
            };
        }

        macro_rules! record_crates {
            ($db:expr, $ex:expr, $($name:expr => ($tc1:expr, $tc2:expr)),*) => {
                {
                    let mut crates = Vec::new();
                    $(
                        let krate = reg!($name);
                        $db.add_dummy_result(
                            &$ex,
                            krate.clone(),
                            MAIN_TOOLCHAIN.clone(),
                            $tc1,
                        );
                        $db.add_dummy_result(
                            &$ex,
                            krate.clone(),
                            TEST_TOOLCHAIN.clone(),
                            $tc2,
                        );
                        crates.push(krate);
                    )*
                    crates
                }
            };
        }

        let config = Config::default();
        let mut db = DummyDB::default();
        let ex = Experiment {
            name: "foo".to_string(),
            toolchains: [MAIN_TOOLCHAIN.clone(), TEST_TOOLCHAIN.clone()],
            mode: Mode::BuildAndTest,
            cap_lints: CapLints::Forbid,
            priority: 0,
            created_at: ::chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            github_issue: None,
            status: Status::GeneratingReport,
            assigned_to: None,
            report_url: None,
            ignore_blacklist: false,
            requirement: None,
        };

        let crates = record_crates! {db, ex,
            "test-pass" => (TestResult::TestPass, TestResult::TestPass),
            "ce-1" => (TestResult::TestPass, TestResult::BuildFail(CompilerError(btreeset!["001".parse()?, "002".parse()?]))),
            "ce-2" => (TestResult::TestPass, TestResult::BuildFail(CompilerError(btreeset!["002".parse()?]))),
            "unknown" => (TestResult::TestPass, TestResult::BuildFail(Unknown)),
            "dep-1" => (TestResult::TestPass, TestResult::BuildFail(DependsOn(btreeset![reg!("ce-1"), reg!("unknown")]))),
            "dep-2" => (TestResult::TestPass, TestResult::BuildFail(DependsOn(btreeset![reg!("ce-1"), reg!("ce-2")]))),
            "fix-1" => (TestResult::BuildFail(DependsOn(btreeset![reg!("ce-1"), reg!("ce-2")])), TestResult::TestPass),
            "fix-2" => (TestResult::BuildFail(Unknown), TestResult::TestPass)
        };

        let raw = generate_report(&db, &config, &ex, &crates)?;
        let mut crates = raw
            .crates
            .clone()
            .into_iter()
            .map(|krate| {
                if let Crate::Registry(ref registry_krate) = krate.krate {
                    (registry_krate.name.clone(), krate)
                } else {
                    panic!("invalid crate type")
                }
            })
            .collect::<HashMap<_, _>>();
        let analyzed = analyze_report(raw);

        let mut info = HashMap::new();
        info.insert(Comparison::Regressed, 5);
        info.insert(Comparison::Fixed, 2);
        info.insert(Comparison::SameTestPass, 1);

        macro_rules! create_results {
            ($src:expr, $($key:expr => ($($krate:expr),*)),*) => {
                {
                    let mut map = HashMap::new();
                    $(
                        let mut crates = Vec::new();
                        $(
                            crates.push($src.get($krate).unwrap().clone());
                        )*
                        map.insert($key, crates);
                    )*
                    map
                }
            }
        }

        let regr_tree = create_results! {crates,
            reg!("ce-1") => ("dep-1", "dep-2"),
            reg!("ce-2") => ("dep-2"),
            reg!("unknown") => ("dep-1")
        };

        let regr_root = create_results! {crates,
            TestResult::BuildFail(CompilerError(btreeset!["001".parse()?])) => ("ce-1"),
            TestResult::BuildFail(CompilerError(btreeset!["002".parse()?])) => ("ce-1", "ce-2"),
            TestResult::BuildFail(Unknown) => ("unknown")
        };

        let regressed = ReportCrates::Complete {
            tree: regr_tree,
            results: regr_root,
        };

        let fix_tree = create_results! {crates,
            reg!("ce-1") => ("fix-1"),
            reg!("ce-2") => ("fix-1")
        };

        let fix_root = create_results! {crates,
            TestResult::BuildFail(Unknown) => ("fix-2")
        };

        let fixed = ReportCrates::Complete {
            tree: fix_tree,
            results: fix_root,
        };

        let test_pass = ReportCrates::Plain(vec![crates.remove("test-pass").unwrap()]);

        let mut categories = HashMap::new();
        categories.insert(Comparison::Regressed, regressed);
        categories.insert(Comparison::Fixed, fixed);
        categories.insert(Comparison::SameTestPass, test_pass);

        let expected = TestResults { categories, info };
        assert_eq!(expected, analyzed);

        Ok(())
    }
}
