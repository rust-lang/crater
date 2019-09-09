use crate::prelude::*;
use rand::{self, distributions::Alphanumeric, Rng};
use rusqlite::{Connection, Transaction};
use serde_json;
use std::collections::HashSet;

fn no_args() -> impl Iterator<Item = &'static str> {
    ::std::iter::empty()
}

enum MigrationKind {
    SQL(&'static str),
    Code(Box<dyn Fn(&Transaction) -> ::rusqlite::Result<()>>),
}

fn migrations() -> Vec<(&'static str, MigrationKind)> {
    let mut migrations = Vec::new();

    migrations.push((
        "initial",
        MigrationKind::SQL(
            "
            CREATE TABLE experiments (
                name TEXT PRIMARY KEY,
                mode TEXT NOT NULL,
                cap_lints TEXT NOT NULL,

                toolchain_start TEXT NOT NULL,
                toolchain_end TEXT NOT NULL,

                priority INTEGER NOT NULL,
                created_at DATETIME NOT NULL,
                status TEXT NOT NULL,
                github_issue TEXT,
                github_issue_url TEXT,
                github_issue_number INTEGER,
                assigned_to TEXT,

                FOREIGN KEY (assigned_to) REFERENCES agents(name) ON DELETE SET NULL
            );

            CREATE TABLE experiment_crates (
                experiment TEXT NOT NULL,
                crate TEXT NOT NULL,

                FOREIGN KEY (experiment) REFERENCES experiments(name) ON DELETE CASCADE
            );

            CREATE TABLE results (
                experiment TEXT NOT NULL,
                crate TEXT NOT NULL,
                toolchain TEXT NOT NULL,
                result TEXT NOT NULL,
                log BLOB NOT NULL,

                PRIMARY KEY (experiment, crate, toolchain) ON CONFLICT REPLACE,
                FOREIGN KEY (experiment) REFERENCES experiments(name) ON DELETE CASCADE
            );

            CREATE TABLE shas (
                experiment TEXT NOT NULL,
                org TEXT NOT NULL,
                name TEXT NOT NULL,
                sha TEXT NOT NULL,

                FOREIGN KEY (experiment) REFERENCES experiments(name) ON DELETE CASCADE
            );

            CREATE TABLE agents (
                name TEXT PRIMARY KEY,
                last_heartbeat DATETIME
            );

            CREATE TABLE saved_names (
                issue INTEGER PRIMARY KEY ON CONFLICT REPLACE,
                experiment TEXT NOT NULL
            );
            ",
        ),
    ));

    migrations.push((
        "store_agents_revision",
        MigrationKind::SQL(
            "
            ALTER TABLE agents ADD COLUMN git_revision TEXT;
            ",
        ),
    ));

    migrations.push((
        "store_skipped_crates",
        MigrationKind::SQL(
            "
            ALTER TABLE experiment_crates ADD COLUMN skipped INTEGER NOT NULL DEFAULT 0;
            ",
        ),
    ));

    migrations.push((
        "add_ui_progress_percent_indexes",
        MigrationKind::SQL(
            "
            CREATE INDEX experiment_crates__experiment_skipped
            ON experiment_crates (experiment, skipped);

            CREATE INDEX results__experiment
            ON results (experiment);
            ",
        ),
    ));

    migrations.push((
        "add_more_experiment_dates",
        MigrationKind::SQL(
            "
            ALTER TABLE experiments ADD COLUMN started_at DATETIME;
            ALTER TABLE experiments ADD COLUMN completed_at DATETIME;
            ",
        ),
    ));

    migrations.push((
        "store_report_url",
        MigrationKind::SQL(
            "
            ALTER TABLE experiments ADD COLUMN report_url TEXT;
            ",
        ),
    ));

    migrations.push((
        "stringify_toolchain_names",
        MigrationKind::Code(Box::new(|t| {
            #[derive(Deserialize)]
            enum LegacyToolchain {
                Dist(String),
                TryBuild { sha: String },
                Master { sha: String },
            }

            let fn_name = format!(
                "crater_migration__{}",
                rand::thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(10)
                    .collect::<String>()
            );
            t.create_scalar_function(&fn_name, 1, true, |ctx| {
                let legacy = ctx.get::<String>(0)?;

                if let Ok(parsed) = serde_json::from_str(&legacy) {
                    Ok(match parsed {
                        LegacyToolchain::Dist(name) => name,
                        LegacyToolchain::TryBuild { sha } => format!("try#{}", sha),
                        LegacyToolchain::Master { sha } => format!("master#{}", sha),
                    })
                } else {
                    Ok(legacy)
                }
            })?;

            t.execute("PRAGMA foreign_keys = OFF;", no_args())?;
            t.execute(
                &format!(
                    "UPDATE experiments SET toolchain_start = {}(toolchain_start);",
                    fn_name
                ),
                no_args(),
            )?;
            t.execute(
                &format!(
                    "UPDATE experiments SET toolchain_end = {}(toolchain_end);",
                    fn_name
                ),
                no_args(),
            )?;
            t.execute(
                &format!("UPDATE results SET toolchain = {}(toolchain);", fn_name),
                no_args(),
            )?;
            t.execute("PRAGMA foreign_keys = ON;", no_args())?;

            Ok(())
        })),
    ));

    migrations.push((
        "migrate_agents_to_assignees_in_experiments",
        MigrationKind::SQL(
            "
            CREATE TABLE experiments_new (
                name TEXT PRIMARY KEY,
                mode TEXT NOT NULL,
                cap_lints TEXT NOT NULL,

                toolchain_start TEXT NOT NULL,
                toolchain_end TEXT NOT NULL,

                created_at DATETIME NOT NULL,
                started_at DATETIME,
                completed_at DATETIME,

                priority INTEGER NOT NULL,
                status TEXT NOT NULL,
                report_url TEXT,
                github_issue TEXT,
                github_issue_url TEXT,
                github_issue_number INTEGER,
                assigned_to TEXT
            );

            INSERT INTO experiments_new (
                name, mode, cap_lints, toolchain_start, toolchain_end, created_at, started_at,
                completed_at, priority, status, report_url, github_issue, github_issue_url,
                github_issue_number, assigned_to
            ) SELECT
                name, mode, cap_lints, toolchain_start, toolchain_end, created_at, started_at,
                completed_at, priority, status, report_url, github_issue, github_issue_url,
                github_issue_number,
                CASE WHEN assigned_to IS NOT NULL THEN 'agent:' || assigned_to ELSE NULL END
            FROM experiments;

            DROP TABLE experiments;
            ALTER TABLE experiments_new RENAME TO experiments;
            ",
        ),
    ));

    migrations.push((
        "create_crates_table",
        MigrationKind::SQL(
            "
            CREATE TABLE crates (
                crate TEXT NOT NULL,
                list TEXT NOT NULL,
                loaded_at DATETIME NOT NULL,

                PRIMARY KEY (crate, list) ON CONFLICT REPLACE
            );

            CREATE INDEX crates__list ON crates (list);
            ",
        ),
    ));

    migrations.push((
        "add_experiment_field_ignore_blacklist",
        MigrationKind::SQL(
            "
            ALTER TABLE experiments ADD COLUMN ignore_blacklist INTEGER NOT NULL DEFAULT 0;
            ",
        ),
    ));

    migrations.push((
        "add_encoding_type",
        MigrationKind::SQL(
            "
            ALTER TABLE results ADD COLUMN encoding TEXT NOT NULL DEFAULT 'plain'
            ",
        ),
    ));

    migrations.push((
        "add_try_builds_table",
        MigrationKind::SQL(
            "
            CREATE TABLE try_builds (
                pr INTEGER NOT NULL,
                repo STRING NOT NULL,
                merge_sha TEXT NOT NULL,
                base_sha TEXT NOT NULL,

                PRIMARY KEY (pr, repo)
            );
            ",
        ),
    ));

    migrations.push((
        "add_experiment_field_requirement",
        MigrationKind::SQL(
            "
            ALTER TABLE experiments ADD COLUMN requirement TEXT DEFAULT 'linux';
            ",
        ),
    ));

    migrations.push((
        "add_agent_capabilities",
        MigrationKind::SQL(
            "
            CREATE TABLE agent_capabilities (
                agent_name TEXT NOT NULL,
                capability TEXT NOT NULL,

                PRIMARY KEY (agent_name, capability) ON CONFLICT REPLACE,
                FOREIGN KEY (agent_name) REFERENCES agents(name) ON DELETE CASCADE
            );

            CREATE INDEX agent__name ON agent_capabilities(agent_name);
            ",
        ),
    ));

    migrations
}

pub fn execute(db: &mut Connection) -> Fallible<()> {
    // If the database version is 0, create the migrations table and bump it
    let version: i32 = db.query_row("PRAGMA user_version;", no_args(), |r| r.get(0))?;
    if version == 0 {
        db.execute(
            "CREATE TABLE migrations (name TEXT PRIMARY KEY);",
            no_args(),
        )?;
        db.execute("PRAGMA user_version = 1;", no_args())?;
    }

    let executed_migrations = {
        let mut prepared = db.prepare("SELECT name FROM migrations;")?;
        let mut result = HashSet::new();
        for value in prepared.query_map(no_args(), |row| -> String { row.get("name") })? {
            result.insert(value?);
        }

        result
    };

    for &(name, ref migration) in &migrations() {
        if !executed_migrations.contains(&name.to_string()) {
            let t = db.transaction()?;
            match migration {
                MigrationKind::SQL(sql) => t.execute_batch(sql),
                MigrationKind::Code(code) => code(&t),
            }
            .with_context(|_| format!("error running migration: {}", name))?;

            t.execute("INSERT INTO migrations (name) VALUES (?1)", &[&name])?;
            t.commit()?;

            info!("executed migration: {}", name);
        }
    }

    Ok(())
}
