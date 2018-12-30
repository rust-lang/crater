use crate::db::{Database, QueryUtils};
use crate::experiments::{CapLints, CrateSelect, Experiment, GitHubIssue, Mode, Status};
use crate::prelude::*;
use crate::server::github::Issue;
use crate::server::messages::{Label, Message};
use crate::server::routes::webhooks::args::{AbortArgs, EditArgs, RetryReportArgs, RunArgs};
use crate::server::Data;

pub fn ping(data: &Data, issue: &Issue) -> Fallible<()> {
    Message::new()
        .line("ping_pong", "**Pong!**")
        .send(&issue.url, data)?;

    Ok(())
}

pub fn run(host: &str, data: &Data, issue: &Issue, args: RunArgs) -> Fallible<()> {
    let name = setup_run_name(&data.db, issue, args.name)?;

    crate::actions::CreateExperiment {
        name: name.clone(),
        toolchains: [
            args.start
                .ok_or_else(|| err_msg("missing start toolchain"))?,
            args.end.ok_or_else(|| err_msg("missing end toolchain"))?,
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
        ignore_blacklist: args.ignore_blacklist.unwrap_or(false),
    }
    .apply(&data.db, &data.config)?;

    Message::new()
        .line(
            "ok_hand",
            format!(
                "Experiment **`{}`** created and queued.", name
            ),
        )
        .line(
            "mag",
            format!(
                "You can check out [the queue](https://{}) and [this experiment's details](https://{0}/ex/{1}).", host, name
            ),
        ).set_label(Label::ExperimentQueued)
        .send(&issue.url, data)?;

    Ok(())
}

pub fn edit(data: &Data, issue: &Issue, args: EditArgs) -> Fallible<()> {
    let name = get_name(&data.db, issue, args.name)?;

    let changed = crate::actions::EditExperiment {
        name: name.clone(),
        toolchains: [args.start, args.end],
        crates: args.crates,
        mode: args.mode,
        cap_lints: args.cap_lints,
        priority: args.priority,
        ignore_blacklist: args.ignore_blacklist,
    }
    .apply(&data.db, &data.config)?;

    if changed {
        Message::new()
            .line(
                "memo",
                format!("Configuration of the **`{}`** experiment changed.", name),
            )
            .send(&issue.url, data)?;
    } else {
        Message::new()
            .line("warning", "No changes requested.")
            .send(&issue.url, data)?;
    }

    Ok(())
}

pub fn retry_report(data: &Data, issue: &Issue, args: RetryReportArgs) -> Fallible<()> {
    let name = get_name(&data.db, issue, args.name)?;

    if let Some(mut experiment) = Experiment::get(&data.db, &name)? {
        if experiment.status != Status::ReportFailed {
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
            )
            .set_label(Label::ExperimentQueued)
            .send(&issue.url, data)?;

        Ok(())
    } else {
        bail!("an experiment named **`{}`** doesn't exist!", name);
    }
}

pub fn abort(data: &Data, issue: &Issue, args: AbortArgs) -> Fallible<()> {
    let name = get_name(&data.db, issue, args.name)?;

    crate::actions::DeleteExperiment { name: name.clone() }.apply(&data.db, &data.config)?;

    Message::new()
        .line("wastebasket", format!("Experiment **`{}`** deleted!", name))
        .set_label(Label::ExperimentCompleted)
        .send(&issue.url, data)?;

    Ok(())
}

pub fn reload_acl(data: &Data, issue: &Issue) -> Fallible<()> {
    data.acl.refresh_cache(&data.github)?;

    Message::new()
        .line("hammer_and_wrench", "List of authorized users reloaded!")
        .send(&issue.url, data)?;

    Ok(())
}

fn get_name(db: &Database, issue: &Issue, name: Option<String>) -> Fallible<String> {
    if let Some(name) = name {
        store_experiment_name(db, issue, &name)?;
        Ok(name)
    } else if let Some(default) = default_experiment_name(db, issue)? {
        Ok(default)
    } else {
        bail!("missing experiment name");
    }
}

fn store_experiment_name(db: &Database, issue: &Issue, name: &str) -> Fallible<()> {
    // Store the provided experiment name to provide it automatically on next command
    // We don't have to worry about conflicts here since the table is defined with
    // ON CONFLICT IGNORE.
    db.execute(
        "INSERT INTO saved_names (issue, experiment) VALUES (?1, ?2);",
        &[&issue.number, &name],
    )?;
    Ok(())
}

fn default_experiment_name(db: &Database, issue: &Issue) -> Fallible<Option<String>> {
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

/// Set up the name for a new run's experiment, including auto-incrementing generated names and
/// storing experiment names in the database.
fn setup_run_name(db: &Database, issue: &Issue, name: Option<String>) -> Fallible<String> {
    let name = if let Some(name) = name {
        name
    } else {
        generate_new_experiment_name(&db, &issue)?
    };
    store_experiment_name(&db, issue, &name)?;
    Ok(name)
}

/// Automatically generate experiment name, auto-incrementing to the first one which does not
/// exist.  E.g. if this function is passed the an issue `12345`, and experiment `pr-12345`
/// exists, then this command returns Ok("pr-12345-1"). Does not store the result in the database.
fn generate_new_experiment_name(db: &Database, issue: &Issue) -> Fallible<String> {
    let mut name = format!("pr-{}", issue.number);
    let mut idx = 1u16;
    while Experiment::exists(&db, &name)? {
        name = format!("pr-{}-{}", issue.number, idx);
        idx = idx
            .checked_add(1)
            .ok_or_else(|| err_msg("too many similarly-named pull requests"))?;
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::{
        default_experiment_name, generate_new_experiment_name, get_name, setup_run_name,
        store_experiment_name,
    };
    use crate::db::Database;
    use crate::prelude::*;
    use crate::server::github;

    /// Simulate to the `run` command, and return experiment name
    fn dummy_run(db: &Database, issue: &github::Issue, name: Option<String>) -> Fallible<String> {
        let name = setup_run_name(db, issue, name)?;
        crate::actions::CreateExperiment::dummy(&name)
            .apply(&db, &crate::config::Config::default())?;
        Ok(name)
    }

    /// Simulate to the `edit` command, and return experiment name
    fn dummy_edit(db: &Database, issue: &github::Issue, name: Option<String>) -> Fallible<String> {
        let name = get_name(db, issue, name)?;
        crate::actions::EditExperiment::dummy(&name)
            .apply(&db, &crate::config::Config::default())?;
        Ok(name)
    }

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

    #[test]
    fn test_run() {
        let db = Database::temp().unwrap();

        let pr1 = github::Issue {
            number: 1,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: Some(github::PullRequest {
                html_url: String::new(),
            }),
        };
        // test with supplied name
        assert_eq!(
            dummy_run(&db, &pr1, Some("pr-1".to_owned())).expect("dummy run failed"),
            "pr-1"
        );
        // make sure it fails the second time
        assert!(dummy_run(&db, &pr1, Some("pr-1".to_owned())).is_err(),);

        let pr2 = github::Issue {
            number: 2,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: Some(github::PullRequest {
                html_url: String::new(),
            }),
        };
        // test with default-generated name
        assert_eq!(
            dummy_run(&db, &pr2, None).expect("dummy run failed"),
            "pr-2"
        );
        // make sure it increments correctly
        assert_eq!(
            dummy_run(&db, &pr2, None).expect("dummy run failed"),
            "pr-2-1"
        );
        // make sure we don't get e.g. pr-2-1-1
        assert_eq!(
            dummy_run(&db, &pr2, None).expect("dummy run failed"),
            "pr-2-2"
        );
        // make sure we can manually supply name and then continue incrementing
        assert_eq!(
            dummy_run(&db, &pr1, Some("pr-2-custom".to_owned())).expect("dummy run failed"),
            "pr-2-custom"
        );
        assert_eq!(
            dummy_run(&db, &pr2, None).expect("dummy run failed"),
            "pr-2-3"
        );
    }

    #[test]
    fn test_edit() {
        let db = Database::temp().unwrap();

        // test retrieval of name generated in a supplied-name run
        let pr1 = github::Issue {
            number: 1,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: Some(github::PullRequest {
                html_url: String::new(),
            }),
        };
        assert_eq!(
            dummy_run(&db, &pr1, Some("pr-1-custom".to_owned())).expect("dummy run failed"),
            "pr-1-custom"
        );
        assert_eq!(
            dummy_edit(&db, &pr1, None).expect("dummy edit failed"),
            "pr-1-custom"
        );

        // test retrieval of name generated in an auto-generated run
        let pr2 = github::Issue {
            number: 2,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: Some(github::PullRequest {
                html_url: String::new(),
            }),
        };
        assert_eq!(
            dummy_run(&db, &pr2, None).expect("dummy run failed"),
            "pr-2"
        );
        // make sure edit doesn't change name
        assert_eq!(
            dummy_edit(&db, &pr2, None).expect("dummy edit failed"),
            "pr-2"
        );
        // test idempotence
        assert_eq!(
            dummy_edit(&db, &pr2, None).expect("dummy edit failed"),
            "pr-2"
        );
        // test that name incrementing is reflected here
        assert_eq!(
            dummy_run(&db, &pr2, None).expect("dummy run failed"),
            "pr-2-1"
        );
        assert_eq!(
            dummy_edit(&db, &pr2, None).expect("dummy edit failed"),
            "pr-2-1"
        );
    }

    #[test]
    fn test_generate_new_experiment_name() {
        let db = Database::temp().unwrap();
        let pr = github::Issue {
            number: 12345,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: Some(github::PullRequest {
                html_url: String::new(),
            }),
        };

        crate::actions::CreateExperiment::dummy("pr-12345")
            .apply(&db, &crate::config::Config::default())
            .expect("could not store dummy experiment");
        let new_name = generate_new_experiment_name(&db, &pr).unwrap();
        assert_eq!(new_name, "pr-12345-1");
        crate::actions::CreateExperiment::dummy("pr-12345-1")
            .apply(&db, &crate::config::Config::default())
            .expect("could not store dummy experiment");;
        assert_eq!(
            &generate_new_experiment_name(&db, &pr).unwrap(),
            "pr-12345-2"
        );
    }
}
