use assets;
use errors::*;
use ex::Experiment;
use mime;
use minifier;
use report::{Comparison, ReportWriter, TestResults};
use std::collections::HashMap;

fn calculate_summary(res: &TestResults) -> HashMap<Comparison, usize> {
    let mut result = HashMap::new();

    for krate in &res.crates {
        let mut counter = result.entry(krate.res).or_insert(0);
        *counter += 1;
    }

    result
}

#[derive(Serialize)]
struct Context<'a> {
    ex: &'a Experiment,
    res: &'a TestResults,
    static_url: String,
    summary: HashMap<Comparison, usize>,
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
    let crates_count = res.crates.len();

    // Reduce the number of crates if this is the summary
    let summary_res;
    let mut res = res;
    if !full {
        summary_res = TestResults {
            crates: res
                .crates
                .iter()
                .filter(|c| c.res.show_in_summary())
                .cloned()
                .collect(),
        };
        res = &summary_res;
    }

    let context = Context {
        ex,
        res,
        static_url: String::new(),
        summary: calculate_summary(res),
        full,
        crates_count,
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
