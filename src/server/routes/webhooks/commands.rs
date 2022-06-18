use crate::actions::{self, Action, ActionsCtx};
use crate::db::{Database, QueryUtils};
use crate::experiments::{CapLints, CrateSelect, Experiment, GitHubIssue, Mode, Status};
use crate::prelude::*;
use crate::server::github::{GitHub, Issue, Repository};
use crate::server::messages::{Label, Message};
use crate::server::routes::webhooks::args::{
    AbortArgs, CheckArgs, EditArgs, RetryArgs, RetryReportArgs, RunArgs,
};
use crate::server::{Data, GithubData};
use crate::toolchain::Toolchain;
use rustwide::Toolchain as RustwideToolchain;

pub fn ping(data: &Data, github_data: &GithubData, issue: &Issue) -> Fallible<()> {
    Message::new()
        .line("ping_pong", "**Pong!**")
        .send(&issue.url, data, github_data)?;

    Ok(())
}

pub fn check(
    host: &str,
    data: &Data,
    github_data: &GithubData,
    repo: &Repository,
    issue: &Issue,
    args: CheckArgs,
) -> Fallible<()> {
    run(
        host,
        data,
        github_data,
        repo,
        issue,
        RunArgs {
            mode: Some(Mode::CheckOnly),
            name: args.name,
            start: args.start,
            end: args.end,
            crates: args.crates,
            cap_lints: args.cap_lints,
            priority: args.priority,
            ignore_blacklist: args.ignore_blacklist,
            assign: args.assign,
            requirement: args.requirement,
        },
    )
}

pub fn run(
    host: &str,
    data: &Data,
    github_data: &GithubData,
    repo: &Repository,
    issue: &Issue,
    args: RunArgs,
) -> Fallible<()> {
    let name = setup_run_name(&data.db, issue, args.name)?;

    let mut message = Message::new().line(
        "ok_hand",
        format!("Experiment **`{}`** created and queued.", name),
    );

    // Autodetect toolchains only if none of them was specified
    let (mut detected_start, mut detected_end) = (None, None);
    if args.start.is_none() && args.end.is_none() {
        if let Some(build) =
            crate::server::try_builds::get_sha(&data.db, &repo.full_name, issue.number)?
        {
            detected_start = Some(Toolchain {
                source: RustwideToolchain::ci(&build.base_sha, false),
                rustflags: None,
                cargoflags: None,
                ci_try: false,
                patches: Vec::new(),
            });
            detected_end = Some(Toolchain {
                source: RustwideToolchain::ci(&build.merge_sha, false),
                rustflags: None,
                cargoflags: None,
                ci_try: true,
                patches: Vec::new(),
            });
            message = message.line(
                "robot",
                format!("Automatically detected try build {}", build.merge_sha),
            );
            let pr_head = github_data
                .api
                .get_pr_head_sha(&repo.full_name, issue.number)?;
            let mut merge_commit = github_data
                .api
                .get_commit(&repo.full_name, &build.merge_sha)?;
            if merge_commit.parents.len() == 2 {
                // The first parent is the rust-lang/rust commit, and the second
                // parent (index 1) is the PR commit
                let old_pr_head = merge_commit.parents.remove(1).sha;
                if pr_head != old_pr_head {
                    message = message.line(
                        "warning",
                        format!(
                            "Try build based on commit {}, but latest commit is {}. Did you forget to make a new try build?",
                            old_pr_head, pr_head
                        ),
                    );
                }
            } else {
                message = message.line(
                    "warning",
                    format!("Unexpected parents for merge commit {}", build.merge_sha),
                );
            }
        }
    }

    // Make crater runs created via webhook require linux by default.
    let requirement = args.requirement.unwrap_or_else(|| "linux".to_string());
    let crates = args
        .crates
        .map(|c| c.resolve())
        .transpose()
        .map_err(|e| e.context("Failed to resolve crate list"))?;

    actions::CreateExperiment {
        name: name.clone(),
        toolchains: [
            args.start
                .or(detected_start)
                .ok_or_else(|| err_msg("missing start toolchain"))?,
            args.end
                .or(detected_end)
                .ok_or_else(|| err_msg("missing end toolchain"))?,
        ],
        mode: args.mode.unwrap_or(Mode::BuildAndTest),
        crates: crates.unwrap_or(CrateSelect::Full),
        cap_lints: args.cap_lints.unwrap_or(CapLints::Forbid),
        priority: args.priority.unwrap_or(0),
        github_issue: Some(GitHubIssue {
            api_url: issue.url.clone(),
            html_url: issue.html_url.clone(),
            number: issue.number,
        }),
        ignore_blacklist: args.ignore_blacklist.unwrap_or(false),
        assign: args.assign,
        requirement: Some(requirement),
    }
    .apply(&ActionsCtx::new(&data.db, &data.config))?;

    message
        .line(
            "mag",
            format!(
                "You can check out [the queue](https://{}) and [this experiment's details](https://{0}/ex/{1}).", host, name
            ),
        ).set_label(Label::ExperimentQueued)
        .send(&issue.url, data,github_data)?;

    Ok(())
}

pub fn edit(data: &Data, github_data: &GithubData, issue: &Issue, args: EditArgs) -> Fallible<()> {
    let name = get_name(&data.db, issue, args.name)?;

    let crates = args
        .crates
        .map(|c| c.resolve())
        .transpose()
        .map_err(|e| e.context("Failed to resolve crate list"))?;

    actions::EditExperiment {
        name: name.clone(),
        toolchains: [args.start, args.end],
        crates,
        mode: args.mode,
        cap_lints: args.cap_lints,
        priority: args.priority,
        ignore_blacklist: args.ignore_blacklist,
        assign: args.assign,
        requirement: args.requirement,
    }
    .apply(&ActionsCtx::new(&data.db, &data.config))?;

    Message::new()
        .line(
            "memo",
            format!("Configuration of the **`{}`** experiment changed.", name),
        )
        .send(&issue.url, data, github_data)?;

    Ok(())
}

pub fn retry_report(
    data: &Data,
    github_data: &GithubData,
    issue: &Issue,
    args: RetryReportArgs,
) -> Fallible<()> {
    let name = get_name(&data.db, issue, args.name)?;

    if let Some(mut experiment) = Experiment::get(&data.db, &name)? {
        if experiment.status != Status::ReportFailed
            && experiment.status != Status::GeneratingReport
        {
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
            .send(&issue.url, data, github_data)?;

        Ok(())
    } else {
        bail!("an experiment named **`{}`** doesn't exist!", name);
    }
}

pub fn retry(
    data: &Data,
    github_data: &GithubData,
    issue: &Issue,
    args: RetryArgs,
) -> Fallible<()> {
    let name = get_name(&data.db, issue, args.name)?;

    if let Some(mut experiment) = Experiment::get(&data.db, &name)? {
        experiment.set_status(&data.db, Status::Queued)?;
        data.reports_worker.wake();

        Message::new()
            .line(
                "hammer_and_wrench",
                format!("Experiment **`{}`** queued again.", name),
            )
            .set_label(Label::ExperimentQueued)
            .send(&issue.url, data, github_data)?;

        Ok(())
    } else {
        bail!("an experiment named **`{}`** doesn't exist!", name);
    }
}

pub fn abort(
    data: &Data,
    github_data: &GithubData,
    issue: &Issue,
    args: AbortArgs,
) -> Fallible<()> {
    let name = get_name(&data.db, issue, args.name)?;

    actions::DeleteExperiment { name: name.clone() }
        .apply(&ActionsCtx::new(&data.db, &data.config))?;

    Message::new()
        .line("wastebasket", format!("Experiment **`{}`** deleted!", name))
        .set_label(Label::ExperimentCompleted)
        .send(&issue.url, data, github_data)?;

    Ok(())
}

pub fn reload_acl(data: &Data, github_data: &GithubData, issue: &Issue) -> Fallible<()> {
    data.acl.refresh_cache(&github_data.api)?;

    Message::new()
        .line("hammer_and_wrench", "List of authorized users reloaded!")
        .send(&issue.url, data, github_data)?;

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
        generate_new_experiment_name(db, issue)?
    };
    store_experiment_name(db, issue, &name)?;
    Ok(name)
}

/// Automatically generate experiment name, auto-incrementing to the first one which does not
/// exist.  E.g. if this function is passed the an issue `12345`, and experiment `pr-12345`
/// exists, then this command returns Ok("pr-12345-1"). Does not store the result in the database.
fn generate_new_experiment_name(db: &Database, issue: &Issue) -> Fallible<String> {
    let mut name = format!("pr-{}", issue.number);
    let mut idx = 1u16;
    while Experiment::exists(db, &name)? {
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
    use crate::actions::{self, Action, ActionsCtx};
    use crate::config::Config;
    use crate::db::Database;
    use crate::prelude::*;
    use crate::server::github;

    /// Simulate to the `run` command, and return experiment name
    fn dummy_run(db: &Database, issue: &github::Issue, name: Option<String>) -> Fallible<String> {
        let config = Config::default();
        let ctx = ActionsCtx::new(db, &config);
        let name = setup_run_name(db, issue, name)?;
        actions::CreateExperiment::dummy(&name).apply(&ctx)?;
        Ok(name)
    }

    /// Simulate to the `edit` command, and return experiment name
    fn dummy_edit(db: &Database, issue: &github::Issue, name: Option<String>) -> Fallible<String> {
        let config = Config::default();
        let ctx = ActionsCtx::new(db, &config);
        let name = get_name(db, issue, name)?;
        actions::EditExperiment::dummy(&name).apply(&ctx)?;
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
        let config = Config::default();
        let ctx = ActionsCtx::new(&db, &config);

        let pr = github::Issue {
            number: 12345,
            url: String::new(),
            html_url: String::new(),
            labels: Vec::new(),
            pull_request: Some(github::PullRequest {
                html_url: String::new(),
            }),
        };

        actions::CreateExperiment::dummy("pr-12345")
            .apply(&ctx)
            .expect("could not store dummy experiment");
        let new_name = generate_new_experiment_name(&db, &pr).unwrap();
        assert_eq!(new_name, "pr-12345-1");
        actions::CreateExperiment::dummy("pr-12345-1")
            .apply(&ctx)
            .expect("could not store dummy experiment");
        assert_eq!(
            &generate_new_experiment_name(&db, &pr).unwrap(),
            "pr-12345-2"
        );
    }
}
