use crate::prelude::*;
use crate::report::Comparison;
use crate::results::{BrokenReason, FailureReason, TestResult};

pub trait ResultName {
    fn short_name(&self) -> String;
    fn long_name(&self) -> String;
}

impl ResultName for FailureReason {
    fn short_name(&self) -> String {
        match self {
            FailureReason::Unknown => "failed (unknown)".into(),
            FailureReason::Timeout => "timed out".into(),
            FailureReason::OOM => "OOM".into(),
            FailureReason::ICE => "ICE".into(),
            FailureReason::CompilerError(_) => "compiler error".into(),
            FailureReason::DependsOn(_) => "faulty deps".into(),
        }
    }

    fn long_name(&self) -> String {
        match self {
            FailureReason::CompilerError(_) | FailureReason::DependsOn(_) => self.to_string(),
            FailureReason::Unknown
            | FailureReason::Timeout
            | FailureReason::OOM
            | FailureReason::ICE => self.short_name(),
        }
    }
}

impl ResultName for BrokenReason {
    fn short_name(&self) -> String {
        match self {
            BrokenReason::Unknown => "broken crate".into(),
            BrokenReason::CargoToml => "broken Cargo.toml".into(),
            BrokenReason::Yanked => "deps yanked".into(),
            BrokenReason::MissingGitRepository => "missing repo".into(),
        }
    }

    fn long_name(&self) -> String {
        self.short_name()
    }
}

impl ResultName for TestResult {
    fn short_name(&self) -> String {
        match self {
            TestResult::BrokenCrate(reason) => reason.short_name(),
            TestResult::BuildFail(reason) => format!("build {}", reason.short_name()),
            TestResult::TestFail(reason) => format!("test {}", reason.short_name()),
            TestResult::TestSkipped => "test skipped".into(),
            TestResult::TestPass => "test passed".into(),
            TestResult::Error => "error".into(),
            TestResult::Skipped => "skipped".into(),
        }
    }

    fn long_name(&self) -> String {
        match self {
            TestResult::BuildFail(reason) => format!("build {}", reason.long_name()),
            TestResult::TestFail(reason) => format!("test {}", reason.long_name()),
            TestResult::BrokenCrate(reason) => reason.long_name(),
            TestResult::TestSkipped
            | TestResult::TestPass
            | TestResult::Error
            | TestResult::Skipped => self.short_name(),
        }
    }
}

#[derive(Serialize)]
pub enum Color {
    Single(&'static str),
    Striped(&'static str, &'static str),
}

pub trait ResultColor {
    fn color(&self) -> Color;
}

impl ResultColor for Comparison {
    fn color(&self) -> Color {
        match self {
            Comparison::Regressed => Color::Single("#db3026"),
            Comparison::Fixed => Color::Single("#5630db"),
            Comparison::Skipped => Color::Striped("#494b4a", "#555555"),
            Comparison::Unknown => Color::Single("#494b4a"),
            Comparison::SameBuildFail => Color::Single("#65461e"),
            Comparison::SameTestFail => Color::Single("#788843"),
            Comparison::SameTestSkipped => Color::Striped("#72a156", "#80b65f"),
            Comparison::SameTestPass => Color::Single("#72a156"),
            Comparison::Error => Color::Single("#d77026"),
            Comparison::Broken => Color::Single("#44176e"),
            Comparison::SpuriousRegressed => Color::Striped("#db3026", "#d5433b"),
            Comparison::SpuriousFixed => Color::Striped("#5630db", "#5d3dcf"),
        }
    }
}

impl ResultColor for TestResult {
    fn color(&self) -> Color {
        match self {
            TestResult::BrokenCrate(_) => Color::Single("#44176e"),
            TestResult::BuildFail(_) => Color::Single("#db3026"),
            TestResult::TestFail(_) => Color::Single("#65461e"),
            TestResult::TestSkipped | TestResult::TestPass => Color::Single("#62a156"),
            TestResult::Error => Color::Single("#d77026"),
            TestResult::Skipped => Color::Single("#494b4a"),
        }
    }
}
