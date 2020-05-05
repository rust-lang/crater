use crate::assets;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::report::{archives::Archive, Comparison, CrateResult, ReportWriter, TestResults};
use crate::results::{BrokenReason, EncodingType, FailureReason, TestResult};
use std::collections::HashMap;

#[derive(Serialize)]
enum Color {
    Single(&'static str),
    Striped(&'static str, &'static str),
}

trait ResultColor {
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
        }
    }
}

trait ResultName {
    fn name(&self) -> String;
}

impl ResultName for FailureReason {
    fn name(&self) -> String {
        match self {
            FailureReason::Unknown => "failed".into(),
            FailureReason::Timeout => "timed out".into(),
            FailureReason::OOM => "OOM".into(),
            FailureReason::ICE => "ICE".into(),
        }
    }
}

impl ResultName for BrokenReason {
    fn name(&self) -> String {
        match self {
            BrokenReason::Unknown => "broken crate".into(),
            BrokenReason::CargoToml => "broken Cargo.toml".into(),
            BrokenReason::Yanked => "deps yanked".into(),
            BrokenReason::MissingGitRepository => "missing repo".into(),
        }
    }
}

impl ResultName for TestResult {
    fn name(&self) -> String {
        match self {
            TestResult::BrokenCrate(reason) => reason.name(),
            TestResult::BuildFail(reason) => format!("build {}", reason.name()),
            TestResult::TestFail(reason) => format!("test {}", reason.name()),
            TestResult::TestSkipped => "test skipped".into(),
            TestResult::TestPass => "test passed".into(),
            TestResult::Error => "error".into(),
        }
    }
}

#[derive(Serialize)]
struct NavbarItem {
    label: &'static str,
    url: &'static str,
    active: bool,
}

#[derive(PartialEq, Eq)]
enum CurrentPage {
    Summary,
    Full,
    Downloads,
}

impl CurrentPage {
    fn navbar(&self) -> Vec<NavbarItem> {
        vec![
            NavbarItem {
                label: "Summary",
                url: "index.html",
                active: *self == CurrentPage::Summary,
            },
            NavbarItem {
                label: "Full report",
                url: "full.html",
                active: *self == CurrentPage::Full,
            },
            NavbarItem {
                label: "Downloads",
                url: "downloads.html",
                active: *self == CurrentPage::Downloads,
            },
        ]
    }
}

#[derive(Serialize)]
struct ResultsContext<'a> {
    ex: &'a Experiment,
    nav: Vec<NavbarItem>,
    categories: HashMap<Comparison, Vec<CrateResult>>,
    full: bool,
    crates_count: usize,

    comparison_colors: HashMap<Comparison, Color>,
    result_colors: HashMap<TestResult, Color>,
    result_names: HashMap<TestResult, String>,
}

#[derive(Serialize)]
struct DownloadsContext<'a> {
    ex: &'a Experiment,
    nav: Vec<NavbarItem>,
    crates_count: usize,

    available_archives: Vec<Archive>,
}

fn write_report<W: ReportWriter>(
    ex: &Experiment,
    crates_count: usize,
    res: &TestResults,
    full: bool,
    to: &str,
    dest: &W,
) -> Fallible<()> {
    let mut comparison_colors = HashMap::new();
    let mut result_colors = HashMap::new();
    let mut result_names = HashMap::new();

    let mut categories = HashMap::new();
    for result in &res.crates {
        // Skip some categories if this is not the full report
        if !full && !result.res.show_in_summary() {
            continue;
        }

        // Add the colors and names used in this run
        comparison_colors
            .entry(result.res)
            .or_insert_with(|| result.res.color());
        if let Some(ref run) = result.runs[0] {
            result_colors
                .entry(run.res)
                .or_insert_with(|| run.res.color());
            result_names
                .entry(run.res)
                .or_insert_with(|| run.res.name());
        }
        if let Some(ref run) = result.runs[1] {
            result_colors
                .entry(run.res)
                .or_insert_with(|| run.res.color());
            result_names
                .entry(run.res)
                .or_insert_with(|| run.res.name());
        }

        let category = categories.entry(result.res).or_insert_with(Vec::new);
        category.push(result.clone());
    }

    let context = ResultsContext {
        ex,
        nav: if full {
            CurrentPage::Full
        } else {
            CurrentPage::Summary
        }
        .navbar(),
        categories,
        full,
        crates_count,
        comparison_colors,
        result_colors,
        result_names,
    };

    info!("generating {}", to);
    let html = minifier::html::minify(&assets::render_template("report/results.html", &context)?);
    dest.write_string(to, html.into(), &mime::TEXT_HTML)?;

    Ok(())
}

fn write_downloads<W: ReportWriter>(
    ex: &Experiment,
    crates_count: usize,
    available_archives: Vec<Archive>,
    dest: &W,
) -> Fallible<()> {
    let context = DownloadsContext {
        ex,
        nav: CurrentPage::Downloads.navbar(),
        crates_count,
        available_archives,
    };

    info!("generating downloads.html");
    let html = minifier::html::minify(&assets::render_template("report/downloads.html", &context)?);
    dest.write_string("downloads.html", html.into(), &mime::TEXT_HTML)?;

    Ok(())
}

pub fn write_html_report<W: ReportWriter>(
    ex: &Experiment,
    crates_count: usize,
    res: &TestResults,
    available_archives: Vec<Archive>,
    dest: &W,
) -> Fallible<()> {
    let js_in = assets::load("report.js")?;
    let css_in = assets::load("report.css")?;
    write_report(ex, crates_count, res, false, "index.html", dest)?;
    write_report(ex, crates_count, res, true, "full.html", dest)?;
    write_downloads(ex, crates_count, available_archives, dest)?;

    info!("copying static assets");
    dest.write_bytes(
        "report.js",
        js_in.content()?.into_owned(),
        js_in.mime(),
        EncodingType::Plain,
    )?;
    dest.write_bytes(
        "report.css",
        css_in.content()?.into_owned(),
        css_in.mime(),
        EncodingType::Plain,
    )?;

    Ok(())
}
