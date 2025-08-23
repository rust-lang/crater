use crate::config::Config;
use crate::crates::sources::github::GitHubRepo;
use crate::crates::{Crate, RegistryCrate};
use crate::db::{Database, QueryUtils};
use crate::experiments::CrateSelect;
use crate::prelude::*;
use chrono::Utc;
use rand::seq::SliceRandom;
use std::collections::HashSet;

pub(crate) use crate::crates::sources::{
    github::GitHubList, local::LocalList, registry::RegistryList,
};

pub(crate) trait List {
    const NAME: &'static str;

    fn fetch(&self) -> Fallible<Vec<Crate>>;

    fn update(&self, db: &Database) -> Fallible<()> {
        let crates = self.fetch()?;

        let now = Utc::now();
        db.transaction(true, |t| {
            // Replace the existing list in the database
            t.execute("DELETE FROM crates WHERE list = ?1;", &[&Self::NAME])?;
            for krate in &crates {
                t.execute(
                    "INSERT INTO crates (crate, list, loaded_at) VALUES (?1, ?2, ?3);",
                    &[&krate.id(), &Self::NAME, &now],
                )
                .with_context(|| {
                    format!(
                        "failed to insert crate {} into the {} list",
                        krate,
                        Self::NAME
                    )
                })?;
            }

            Ok(())
        })?;

        info!("loaded {} crates in the {} list", crates.len(), Self::NAME);
        Ok(())
    }

    fn get(db: &Database) -> Fallible<Vec<Crate>> {
        let crates_results = db.query(
            "SELECT crate FROM crates WHERE list = ?1 ORDER BY rowid;",
            [&Self::NAME],
            |r| r.get::<_, String>(0),
        )?;

        // Turns Vec<Fallible<Crate>> into Fallible<Vec<Crate>>
        crates_results.into_iter().map(|v| v.parse()).collect()
    }
}

pub(crate) fn get_crates(
    select: &CrateSelect,
    db: &Database,
    config: &Config,
) -> Fallible<Vec<Crate>> {
    let mut crates = Vec::new();

    match select {
        CrateSelect::Full => {
            crates.append(&mut RegistryList::get(db)?);
            crates.append(&mut GitHubList::get(db)?);
        }

        CrateSelect::Demo => {
            let mut demo_registry = config
                .demo_crates()
                .crates
                .iter()
                .map(|v| v.as_str())
                .collect::<HashSet<_>>();
            let mut demo_github = config
                .demo_crates()
                .github_repos
                .iter()
                .collect::<HashSet<_>>();
            let mut demo_local = config
                .demo_crates()
                .local_crates
                .iter()
                .collect::<HashSet<_>>();

            let mut all_crates = Vec::new();
            all_crates.append(&mut RegistryList::get(db)?);
            all_crates.append(&mut GitHubList::get(db)?);
            all_crates.append(&mut LocalList::get(db)?);

            for krate in all_crates {
                let add = match krate {
                    Crate::Registry(RegistryCrate { ref name, .. }) => {
                        demo_registry.remove(name.as_str())
                    }
                    Crate::GitHub(ref repo) => demo_github.remove(&repo.slug()),
                    Crate::Local(ref name) => demo_local.remove(name),
                    Crate::Git(_) | Crate::Path(_) => unimplemented!("unsupported crate"),
                };

                if add {
                    crates.push(krate);
                }
            }

            // Do some sanity checks
            if !demo_registry.is_empty() {
                bail!("missing demo crates: {:?}", demo_registry);
            }
            if !demo_github.is_empty() {
                bail!("missing demo GitHub repos: {:?}", demo_github);
            }
            if !demo_local.is_empty() {
                bail!("missing demo local crates: {:?}", demo_local);
            }
        }
        CrateSelect::List(list) => {
            let mut desired = list.clone();

            let mut all_crates = Vec::new();
            all_crates.append(&mut RegistryList::get(db)?);
            all_crates.append(&mut GitHubList::get(db)?);

            for krate in all_crates {
                let is_desired = match krate {
                    Crate::Registry(RegistryCrate { ref name, .. }) => {
                        desired.remove(name.as_str())
                    }
                    Crate::GitHub(ref repo) => desired.remove(&repo.slug()),
                    _ => unreachable!(),
                };

                if is_desired {
                    crates.push(krate);
                }
            }
        }

        CrateSelect::Random(n) => {
            crates.append(&mut RegistryList::get(db)?);
            crates.append(&mut GitHubList::get(db)?);

            let mut rng = rand::rng();
            crates.shuffle(&mut rng);
            crates.truncate(*n as usize);
        }
        CrateSelect::Top(n) => {
            crates.append(&mut RegistryList::get(db)?);
            crates.truncate(*n as usize);
        }
        CrateSelect::Local => {
            crates.append(&mut LocalList::get(db)?);
        }
        CrateSelect::Dummy => crates.push(Crate::GitHub(GitHubRepo::dummy())),
    }

    crates.sort();
    Ok(crates)
}

#[cfg(test)]
pub(crate) fn setup_test_lists(db: &Database, config: &Config) -> Fallible<()> {
    use crate::actions::{Action, ActionsCtx, UpdateLists};

    UpdateLists {
        github: false,
        registry: false,
        local: true,
    }
    .apply(&ActionsCtx::new(db, config))
}
