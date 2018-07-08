use assets;
use errors::*;
use ex::Experiment;
use mime;
use minifier;
use report::{Comparison, CrateResult, ReportWriter, TestResults};
use results::TestResult;
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
        }
    }
}

impl ResultColor for TestResult {
    fn color(&self) -> Color {
        match self {
            TestResult::BuildFail => Color::Single("#db3026"),
            TestResult::TestFail => Color::Single("#65461e"),
            TestResult::TestSkipped | TestResult::TestPass => Color::Single("#62a156"),
            TestResult::Error => Color::Single("#d77026"),
        }
    }
}

#[derive(Serialize)]
struct Context<'a> {
    ex: &'a Experiment,
    static_url: String,
    categories: HashMap<Comparison, Vec<CrateResult>>,
    full: bool,
    crates_count: usize,

    comparison_colors: HashMap<Comparison, Color>,
    result_colors: HashMap<TestResult, Color>,
}

fn write_report<W: ReportWriter>(
    ex: &Experiment,
    res: &TestResults,
    full: bool,
    to: &str,
    dest: &W,
) -> Result<()> {
    let mut comparison_colors = HashMap::new();
    let mut result_colors = HashMap::new();

    let mut categories = HashMap::new();
    for result in &res.crates {
        // Skip some categories if this is not the full report
        if !full && !result.res.show_in_summary() {
            continue;
        }

        // Add the colors used in this run
        comparison_colors
            .entry(result.res)
            .or_insert_with(|| result.res.color());
        if let Some(ref run) = result.runs[0] {
            result_colors
                .entry(run.res)
                .or_insert_with(|| run.res.color());
        }
        if let Some(ref run) = result.runs[1] {
            result_colors
                .entry(run.res)
                .or_insert_with(|| run.res.color());
        }

        let mut category = categories.entry(result.res).or_insert_with(Vec::new);
        category.push(result.clone());
    }

    let context = Context {
        ex,
        static_url: String::new(),
        categories,
        full,
        crates_count: res.crates.len(),

        comparison_colors,
        result_colors,
    };

    info!("generating {}", to);
    let html = minifier::html::minify(&assets::render_template("report.html", &context)?);
    dest.write_string(to, html.into(), &mime::TEXT_HTML)?;

    Ok(())
}

pub fn write_html_report<W: ReportWriter>(
    ex: &Experiment,
    res: &TestResults,
    dest: &W,
) -> Result<()> {
    let js_in = assets::load("report.js")?;
    let css_in = assets::load("report.css")?;
    write_report(ex, res, false, "index.html", dest)?;
    write_report(ex, res, true, "full.html", dest)?;

    info!("copying static assets");
    dest.write_string("report.js", js_in.content()?, js_in.mime())?;
    dest.write_string("report.css", css_in.content()?, css_in.mime())?;

    Ok(())
}
