use std::collections::HashMap;

use crate::assets;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::report::{
    analyzer::ReportCrates, archives::Archive, Color, Comparison, CrateResult, ReportWriter,
    ResultColor, ResultName, TestResults,
};
use crate::results::EncodingType;
use indexmap::{IndexMap, IndexSet};

use super::CrateVersionStatus;

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
enum ReportCratesHTML<'a> {
    Plain(Vec<CrateResultHTML<'a>>),
    Tree {
        count: u32,
        tree: IndexMap<String, Vec<CrateResultHTML<'a>>>,
    },
    RootResults {
        count: u32,
        results: IndexMap<String, Vec<CrateResultHTML<'a>>>,
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
    // (comparison, category color, ...)
    categories: Vec<(Comparison, usize, ReportCratesHTML<'a>)>,
    info: IndexMap<Comparison, u32>,
    full: bool,
    crates_count: usize,
    colors: IndexSet<Color>,
    result_names: IndexSet<String>,
}

#[derive(Serialize)]
struct DownloadsContext<'a> {
    ex: &'a Experiment,
    nav: Vec<NavbarItem>,
    crates_count: usize,

    available_archives: Vec<Archive>,
}

#[derive(Serialize)]
struct CrateResultHTML<'a> {
    name: &'a str,
    url: &'a str,
    res: Comparison,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<CrateVersionStatus>,
    color_idx: usize,
    runs: [Option<BuildTestResultHTML<'a>>; 2],
}

// Map TestResult to usize to avoid the presence of special characters in html
#[derive(Serialize)]
struct BuildTestResultHTML<'a> {
    color_idx: usize,
    name_idx: usize,
    log: &'a str,
}

fn to_html_crate_result<'a>(
    colors: &mut IndexSet<Color>,
    result_names: &mut IndexSet<String>,
    category_color: usize,
    result: &'a CrateResult,
) -> CrateResultHTML<'a> {
    let mut runs = [None, None];

    for (pos, run) in result.runs.iter().enumerate() {
        if let Some(run) = run {
            let (color_idx, _) = colors.insert_full(run.res.color());
            let (name_idx, _) = result_names.insert_full(run.res.short_name());
            runs[pos] = Some(BuildTestResultHTML {
                color_idx,
                name_idx,
                log: run.log.as_str(),
            });
        }
    }

    CrateResultHTML {
        name: result.name.as_str(),
        url: result.url.as_str(),
        status: result.status,
        res: result.res,
        color_idx: category_color,
        runs,
    }
}

fn write_report<W: ReportWriter>(
    ex: &Experiment,
    crates_count: usize,
    res: &TestResults,
    full: bool,
    to: &str,
    dest: &W,
    output_templates: bool,
) -> Fallible<()> {
    let mut colors = IndexSet::new();
    let mut result_names = IndexSet::new();

    let color_for_category = res
        .categories
        .keys()
        .map(|category| (category.color(), colors.insert_full(category.color()).0))
        .collect::<HashMap<_, _>>();

    let categories = res
        .categories
        .iter()
        .filter(|(category, _)| full || category.show_in_summary())
        .map(|(&category, crates)| (category, crates))
        .flat_map(|(category, crates)| {
            let category_color_idx = *color_for_category.get(&category.color()).unwrap();
            match crates {
                ReportCrates::Plain(crates) => vec![(
                    category,
                    category_color_idx,
                    ReportCratesHTML::Plain(
                        crates
                            .iter()
                            .map(|result| {
                                to_html_crate_result(
                                    &mut colors,
                                    &mut result_names,
                                    category_color_idx,
                                    result,
                                )
                            })
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
                                deps.iter()
                                    .map(|result| {
                                        to_html_crate_result(
                                            &mut colors,
                                            &mut result_names,
                                            category_color_idx,
                                            result,
                                        )
                                    })
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
                                    .iter()
                                    .map(|result| {
                                        to_html_crate_result(
                                            &mut colors,
                                            &mut result_names,
                                            category_color_idx,
                                            result,
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<IndexMap<_, _>>();

                    vec![
                        (
                            category,
                            category_color_idx,
                            ReportCratesHTML::Tree {
                                count: tree.keys().len() as u32,
                                tree,
                            },
                        ),
                        (
                            category,
                            category_color_idx,
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
        colors,
        result_names,
    };

    info!("generating {}", to);

    if output_templates {
        dest.write_string(
            [to, ".context.json"].concat(),
            serde_json::to_string(&context)?.into(),
            &mime::APPLICATION_JSON,
        )?;
    }

    let rendered = assets::render_template("report/results.html", context)
        .context("rendering template report/results.html")?;
    let html = minifier::html::minify(&rendered);
    dest.write_string(to, html.into(), &mime::TEXT_HTML)?;

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
        false,
        "index.html",
        dest,
        output_templates,
    )?;
    write_report(
        ex,
        crates_count,
        res,
        true,
        "full.html",
        dest,
        output_templates,
    )?;
    write_downloads(ex, crates_count, available_archives, dest, output_templates)?;

    info!("copying static assets");
    dest.write_bytes(
        "report.js",
        &js_in.content()?,
        js_in.mime(),
        EncodingType::Plain,
    )?;
    dest.write_bytes(
        "report.css",
        &css_in.content()?,
        css_in.mime(),
        EncodingType::Plain,
    )?;

    Ok(())
}
