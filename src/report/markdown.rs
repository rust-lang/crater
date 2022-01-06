use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::report::analyzer::{ReportConfig, ReportCrates, ToolchainSelect};
use crate::report::{
    crate_to_url, BuildTestResult, Comparison, CrateResult, ReportWriter, ResultName, TestResults,
};
use crate::utils::serialize::to_vec;
use indexmap::{IndexMap, IndexSet};
use std::fmt::Write;

#[derive(Serialize)]
enum ReportCratesMD {
    Plain(Vec<CrateResult>),
    Complete {
        // only string keys are allowed in JSON maps
        #[serde(serialize_with = "to_vec")]
        res: IndexMap<CrateResult, Vec<CrateResult>>,
        #[serde(serialize_with = "to_vec")]
        orphans: IndexMap<Crate, Vec<CrateResult>>,
    },
}

#[derive(Serialize)]
struct ResultsContext<'a> {
    ex: &'a Experiment,
    categories: Vec<(Comparison, ReportCratesMD)>,
    info: IndexMap<Comparison, u32>,
    full: bool,
    crates_count: usize,
}

fn write_crate(
    mut rendered: &mut String,
    krate: &CrateResult,
    comparison: Comparison,
    is_child: bool,
) -> Fallible<()> {
    let get_run_name = |run: &BuildTestResult| {
        if !is_child {
            run.res.long_name()
        } else {
            run.res.short_name()
        }
    };

    let runs = [
        krate.runs[0]
            .as_ref()
            .map(get_run_name)
            .unwrap_or_else(|| "unavailable".into()),
        krate.runs[0]
            .as_ref()
            .map(|run| run.log.to_owned())
            .unwrap_or_else(|| "#".into()),
        krate.runs[1]
            .as_ref()
            .map(get_run_name)
            .unwrap_or_else(|| "unavailable".into()),
        krate.runs[1]
            .as_ref()
            .map(|run| run.log.to_owned())
            .unwrap_or_else(|| "#".into()),
    ];

    let prefix = if is_child { "  * " } else { "* " };
    let status_warning = krate
        .status
        .map(|status| format!(" ({})", status))
        .unwrap_or_default();

    if let ReportConfig::Complete(toolchain) = comparison.report_config() {
        let (conj, run) = match toolchain {
            ToolchainSelect::Start => ("from", 0),
            ToolchainSelect::End => ("due to", 2),
        };

        writeln!(
            &mut rendered,
            "{}[{}{}]({}) {} {} **{}** [start]({}/log.txt) | [end]({}/log.txt)",
            prefix,
            krate.name,
            status_warning,
            krate.url,
            comparison,
            conj,
            runs[run],
            runs[1],
            runs[3]
        )?;
    } else {
        writeln!(
            &mut rendered,
            "{}[{}{}]({}) {} [start]({}/log.txt) | [end]({}/log.txt)",
            prefix, krate.name, status_warning, krate.url, comparison, runs[1], runs[3]
        )?;
    };

    Ok(())
}

fn render_markdown(context: &ResultsContext) -> Fallible<String> {
    let mut rendered = String::new();

    //add title
    writeln!(&mut rendered, "# Crater report for {}\n\n", context.ex.name)?;

    for (comparison, results) in context.categories.iter() {
        writeln!(&mut rendered, "\n### {}", comparison)?;
        match results {
            ReportCratesMD::Plain(crates) => {
                for krate in crates {
                    write_crate(&mut rendered, krate, *comparison, false)?;
                }
            }
            ReportCratesMD::Complete { res, orphans } => {
                for (root, deps) in res {
                    write_crate(&mut rendered, root, *comparison, false)?;
                    for krate in deps {
                        write_crate(&mut rendered, krate, *comparison, true)?;
                    }
                }

                for (krate, deps) in orphans {
                    writeln!(
                        &mut rendered,
                        "* [{}]({}) (not covered in crater testing)",
                        krate,
                        crate_to_url(krate)
                    )?;
                    for krate in deps {
                        write_crate(&mut rendered, krate, *comparison, true)?;
                    }
                }
            }
        }
    }

    Ok(rendered)
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
    let categories = res
        .categories
        .iter()
        .filter(|(category, _)| full || category.show_in_summary())
        .map(|(&category, crates)| (category, crates.to_owned()))
        .map(|(category, crates)| match crates {
            ReportCrates::Plain(crates) => (
                category,
                ReportCratesMD::Plain(crates.into_iter().collect::<Vec<_>>()),
            ),
            ReportCrates::Complete { mut tree, results } => {
                let res = results
                    .into_iter()
                    .flat_map(|(_key, values)| values.into_iter())
                    .collect::<IndexSet<_>>() // remove duplicates
                    .into_iter()
                    .map(|krate| {
                        // done here to avoid cloning krate
                        let deps = tree.remove(&krate.krate).unwrap_or_default();
                        (krate, deps)
                    })
                    .collect::<IndexMap<_, _>>();

                (category, ReportCratesMD::Complete { res, orphans: tree })
            }
        })
        .collect();

    let context = ResultsContext {
        ex,
        categories,
        info: res.info.clone(),
        full,
        crates_count,
    };

    let markdown = render_markdown(&context)?;
    info!("generating {}", to);
    dest.write_string(to, markdown.into(), &mime::TEXT_PLAIN)?;

    if output_templates {
        dest.write_string(
            [to, ".context.json"].concat(),
            serde_json::to_string(&context)?.into(),
            &mime::TEXT_PLAIN,
        )?;
    }

    Ok(())
}

pub fn write_markdown_report<W: ReportWriter>(
    ex: &Experiment,
    crates_count: usize,
    res: &TestResults,
    dest: &W,
    output_templates: bool,
) -> Fallible<()> {
    write_report(
        ex,
        crates_count,
        res,
        false,
        "markdown.md",
        dest,
        output_templates,
    )?;
    Ok(())
}
