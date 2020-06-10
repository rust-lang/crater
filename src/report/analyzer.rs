use super::{Comparison, CrateResult, RawTestResults};
use crate::crates::Crate;
use crate::results::{
    FailureReason,
    TestResult::{self, BuildFail},
};
use std::collections::BTreeSet;
use std::collections::HashMap;

#[derive(PartialEq)]
pub enum ReportConfig {
    Simple,
    Complete { toolchain: usize },
}

#[derive(Clone)]
pub enum ReportCrates {
    Plain(Vec<CrateResult>),
    Complete {
        tree: HashMap<Crate, Vec<CrateResult>>,
        results: HashMap<TestResult, Vec<CrateResult>>,
    },
}

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
