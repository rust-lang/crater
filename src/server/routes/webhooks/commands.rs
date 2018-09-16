use db::{Database, QueryUtils};
use errors::*;
use experiments::{CapLints, CrateSelect, ExperimentData, GitHubIssue, Mode, Status};
use server::github::Issue;
use server::messages::{Label, Message};
use server::routes::webhooks::args::{AbortArgs, EditArgs, RetryReportArgs, RunArgs};
use server::Data;

pub fn ping(data: &Data, issue: &Issue) -> Result<()> {
    Message::new()
        .line("ping_pong", "**Pong!**")
        .send(&issue.url, data)?;

    Ok(())
}

pub fn run(data: &Data, issue: &Issue, args: RunArgs) -> Result<()> {
    let name = get_name(&data.db, issue, args.name)?;

    ::actions::CreateExperiment {
        name: name.clone(),
        toolchains: [
            args.start.ok_or_else(|| "missing start toolchain")?,
            args.end.ok_or_else(|| "missing end toolchain")?,
        ],
        mode: args.mode.unwrap_or(Mode::BuildAndTest),
        crates: args.crates.unwrap_or(CrateSelect::Full),
        cap_lints: args.cap_lints.unwrap_or(CapLints::Forbid),
        priority: args.priority.unwrap_or(0),
        github_issue: Some(GitHubIssue {
            api_url: issue.url.clone(),
            html_url: issue.html_url.clone(),
            number: issue.number,
        }),
    }.apply(&data.db, &data.config)?;

    Message::new()
        .line(
            "ok_hand",
            format!("Experiment **`{}`** created and queued.", name),
        ).set_label(Label::ExperimentQueued)
        .send(&issue.url, data)?;

    Ok(())
}

pub fn edit(data: &Data, issue: &Issue, args: EditArgs) -> Result<()> {
    let name = get_name(&data.db, issue, args.name)?;

    let changed = ::actions::EditExperiment {
        name: name.clone(),
        toolchains: [args.start, args.end],
        crates: args.crates,
        mode: args.mode,
        cap_lints: args.cap_lints,
        priority: args.priority,
    }.apply(&data.db, &data.config)?;

    if changed {
        Message::new()
            .line(
                "memo",
                format!("Configuration of the **`{}`** experiment changed.", name),
            ).send(&issue.url, data)?;
    } else {
        Message::new()
            .line("warning", "No changes requested.")
            .send(&issue.url, data)?;
    }

    Ok(())
}

pub fn retry_report(data: &Data, issue: &Issue, args: RetryReportArgs) -> Result<()> {
    let name = get_name(&data.db, issue, args.name)?;

    if let Some(mut experiment) = ExperimentData::get(&data.db, &name)? {
        if experiment.server_data.status != Status::ReportFailed {
            bail!(
                "generation of the report of the **`{}`** experiment didn't fail!",
                name
            );
        }

        experiment.set_status(&data.db, Status::NeedsReport)?;
        data.reports_worker.wake();

        Message::new()
            .line(
                "hammer_and_wrench",
                format!("Generation of the report for **`{}`** queued again.", name),
            ).set_label(Label::ExperimentQueued)
            .send(&issue.url, data)?;

        Ok(())
    } else {
        bail!("an experiment named **`{}`** doesn't exist!", name);
    }
}

pub fn abort(data: &Data, issue: &Issue, args: AbortArgs) -> Result<()> {
    let name = get_name(&data.db, issue, args.name)?;

    ::actions::DeleteExperiment { name: name.clone() }.apply(&data.db, &data.config)?;

    Message::new()
        .line("wastebasket", format!("Experiment **`{}`** deleted!", name))
        .set_label(Label::ExperimentCompleted)
        .send(&issue.url, data)?;

    Ok(())
}

pub fn reload_acl(data: &Data, issue: &Issue) -> Result<()> {
    data.acl.refresh_cache(&data.github)?;

    Message::new()
        .line("hammer_and_wrench", "List of authorized users reloaded!")
        .send(&issue.url, data)?;

    Ok(())
}

fn get_name(db: &Database, issue: &Issue, name: Option<String>) -> Result<String> {
    if let Some(name) = name {
        store_experiment_name(db, issue, &name)?;
        Ok(name)
    } else if let Some(default) = default_experiment_name(db, issue)? {
        Ok(default)
    } else {
        Err("missing experiment name".into())
    }
}

fn store_experiment_name(db: &Database, issue: &Issue, name: &str) -> Result<()> {
    // Store the provided experiment name to provide it automatically on next command
    // We don't have to worry about conflicts here since the table is defined with
    // ON CONFLICT IGNORE.
    db.execute(
        "INSERT INTO saved_names (issue, experiment) VALUES (?1, ?2);",
        &[&issue.number, &name],
    )?;
    Ok(())
}

fn default_experiment_name(db: &Database, issue: &Issue) -> Result<Option<String>> {
    let name = db.get_row(
        "SELECT experiment FROM saved_names WHERE issue = ?1",
        &[&issue.number],
        |r| r.get(0),
    )?;

    Ok(if let Some(name) = name {
        Some(name)
    } else if issue.pull_request.is_some() {
        Some(format!("pr-{}", issue.number))
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::{default_experiment_name, store_experiment_name};
    use db::Database;
    use server::github;

    #[test]
    fn test_default_experiment_name() {
        let db = Database::temp().unwrap();

        // With simple issues no default should be used
        let issue = github::Issue {
            number: 1,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: None,
        };
        assert!(default_experiment_name(&db, &issue).unwrap().is_none());

        // With pull requests pr-{number} should be used
        let pr = github::Issue {
            number: 2,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: Some(github::PullRequest {
                html_url: String::new(),
            }),
        };
        assert_eq!(
            default_experiment_name(&db, &pr).unwrap().unwrap().as_str(),
            "pr-2"
        );

        // With a saved experiment name that name should be returned
        store_experiment_name(&db, &pr, "foo").unwrap();
        assert_eq!(
            default_experiment_name(&db, &pr).unwrap().unwrap().as_str(),
            "foo"
        );
    }
}
