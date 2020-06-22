use crate::assets;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::report::{
    archives::Archive, Color, Comparison, ReportWriter, ResultColor, ResultName, TestResults,
};
use crate::results::EncodingType;
use std::collections::HashMap;

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
    categories: HashMap<Comparison, Vec<CrateResultHTML>>,
    full: bool,
    crates_count: usize,

    comparison_colors: HashMap<Comparison, Color>,
    result_colors: Vec<Color>,
    result_names: Vec<String>,
}

#[derive(Serialize)]
struct DownloadsContext<'a> {
    ex: &'a Experiment,
    nav: Vec<NavbarItem>,
    crates_count: usize,

    available_archives: Vec<Archive>,
}

#[derive(Serialize)]
struct CrateResultHTML {
    name: String,
    url: String,
    res: Comparison,
    runs: [Option<BuildTestResultHTML>; 2],
}

// Map TestResult to usize to avoid the presence of special characters in html
#[derive(Serialize)]
struct BuildTestResultHTML {
    res: usize,
    log: String,
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
    let mut test_results_to_int = HashMap::new();
    let mut result_colors = Vec::new();
    let mut result_names = Vec::new();

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

        let mut runs = [None, None];

        for (pos, run) in result.runs.iter().enumerate() {
            if let Some(ref run) = run {
                let idx = test_results_to_int.entry(&run.res).or_insert_with(|| {
                    result_colors.push(run.res.color());
                    result_names.push(run.res.name());
                    result_names.len() - 1
                });
                runs[pos] = Some(BuildTestResultHTML {
                    res: *idx as usize,
                    log: run.log.clone(),
                });
            }
        }

        let category = categories.entry(result.res).or_insert_with(Vec::new);
        let result = CrateResultHTML {
            name: result.name.clone(),
            url: result.url.clone(),
            res: result.res,
            runs,
        };
        category.push(result);
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
