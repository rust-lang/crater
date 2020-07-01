use crate::assets;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::report::{
    analyzer::ReportCrates, archives::Archive, Color, Comparison, CrateResult, ReportPriority,
    ReportWriter, ResultColor, ResultName, TestResults,
};
use crate::results::EncodingType;
use indexmap::IndexMap;

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

#[derive(Serialize)]
enum ReportCratesHTML {
    Plain(Vec<CrateResultHTML>),
    Tree {
        count: u32,
        tree: IndexMap<String, Vec<CrateResultHTML>>,
    },
    RootResults {
        count: u32,
        results: IndexMap<String, Vec<CrateResultHTML>>,
    },
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
    categories: Vec<(Comparison, ReportCratesHTML)>,
    info: IndexMap<Comparison, u32>,
    full: bool,
    crates_count: usize,
    comparison_colors: IndexMap<Comparison, Color>,
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
    priority: ReportPriority,
    to: &str,
    dest: &W,
    output_templates: bool,
) -> Fallible<()> {
    let mut comparison_colors = IndexMap::new();
    let mut test_results_to_int = IndexMap::new();
    let mut result_colors = Vec::new();
    let mut result_names = Vec::new();

    let mut to_html_crate_result = |result: CrateResult| {
        let mut runs = [None, None];

        for (pos, run) in result.runs.iter().enumerate() {
            if let Some(ref run) = run {
                let idx = test_results_to_int
                    .entry(run.res.clone())
                    .or_insert_with(|| {
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

        CrateResultHTML {
            name: result.name.clone(),
            url: result.url.clone(),
            res: result.res,
            runs,
        }
    };

    let categories = res
        .categories
        .iter()
        .filter(|(category, _)| category.report_priority() >= priority)
        .map(|(&category, crates)| (category, crates.to_owned()))
        .flat_map(|(category, crates)| {
            comparison_colors.insert(category, category.color());

            match crates {
                ReportCrates::Plain(crates) => vec![(
                    category,
                    ReportCratesHTML::Plain(
                        crates
                            .into_iter()
                            .map(|result| to_html_crate_result(result))
                            .collect::<Vec<_>>(),
                    ),
                )]
                .into_iter(),
                ReportCrates::Complete { tree, results } => {
                    let tree = tree
                        .into_iter()
                        .map(|(root, deps)| {
                            (
                                root.to_string(),
                                deps.into_iter()
                                    .map(|result| to_html_crate_result(result))
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<IndexMap<_, _>>();
                    let results = results
                        .into_iter()
                        .map(|(res, krates)| {
                            (
                                res.long_name(),
                                krates
                                    .into_iter()
                                    .map(|result| to_html_crate_result(result))
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<IndexMap<_, _>>();

                    vec![
                        (
                            category,
                            ReportCratesHTML::Tree {
                                count: tree.keys().len() as u32,
                                tree,
                            },
                        ),
                        (
                            category,
                            ReportCratesHTML::RootResults {
                                count: results.keys().len() as u32,
                                results,
                            },
                        ),
                    ]
                    .into_iter()
                }
            }
        })
        .collect();

    let full = priority == ReportPriority::Low;
    let context = ResultsContext {
        ex,
        nav: if full {
            CurrentPage::Full
        } else {
            CurrentPage::Summary
        }
        .navbar(),
        categories,
        info: res.info.clone(),
        full,
        crates_count,
        comparison_colors,
        result_colors,
        result_names,
    };

    info!("generating {}", to);
    let html = minifier::html::minify(&assets::render_template("report/results.html", &context)?);
    dest.write_string(to, html.into(), &mime::TEXT_HTML)?;

    if output_templates {
        dest.write_string(
            [to, ".context.json"].concat(),
            serde_json::to_string(&context)?.into(),
            &mime::APPLICATION_JSON,
        )?;
    }

    Ok(())
}

fn write_downloads<W: ReportWriter>(
    ex: &Experiment,
    crates_count: usize,
    available_archives: Vec<Archive>,
    dest: &W,
    output_templates: bool,
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

    if output_templates {
        dest.write_string(
            "downloads.html.context.json",
            serde_json::to_string(&context)?.into(),
            &mime::APPLICATION_JSON,
        )?;
    }

    Ok(())
}

pub fn write_html_report<W: ReportWriter>(
    ex: &Experiment,
    crates_count: usize,
    res: &TestResults,
    available_archives: Vec<Archive>,
    dest: &W,
    output_templates: bool,
) -> Fallible<()> {
    let js_in = assets::load("report.js")?;
    let css_in = assets::load("report.css")?;
    write_report(
        ex,
        crates_count,
        res,
        ReportPriority::Medium,
        "index.html",
        dest,
        output_templates,
    )?;
    write_report(
        ex,
        crates_count,
        res,
        ReportPriority::Low,
        "full.html",
        dest,
        output_templates,
    )?;
    write_downloads(ex, crates_count, available_archives, dest, output_templates)?;

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
