use assets;
use errors::*;
use ex::Experiment;
use mime;
use minifier;
use report::{Comparison, CrateResult, ReportWriter, TestResults};
use std::collections::HashMap;

#[derive(Serialize)]
struct Context<'a> {
    ex: &'a Experiment,
    static_url: String,
    categories: HashMap<Comparison, Vec<CrateResult>>,
    full: bool,
    crates_count: usize,
}

fn write_report<W: ReportWriter>(
    ex: &Experiment,
    res: &TestResults,
    full: bool,
    to: &str,
    dest: &W,
) -> Result<()> {
    let mut categories = HashMap::new();
    for result in &res.crates {
        // Skip some categories if this is not the full report
        if !full && !result.res.show_in_summary() {
            continue;
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
