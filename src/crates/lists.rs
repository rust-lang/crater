use chrono::Utc;
use config::Config;
use crates::{Crate, RegistryCrate};
use db::{Database, QueryUtils};
use errors::*;
use experiments::CrateSelect;
use rand::{thread_rng, Rng};
use std::collections::HashSet;

pub(crate) use crates::sources::{github::GitHubList, registry::RegistryList};

const SMALL_RANDOM_COUNT: usize = 20;

pub(crate) trait List {
    const NAME: &'static str;

    fn fetch(&self) -> Result<Vec<Crate>>;

    fn update(&self, db: &Database) -> Result<()> {
        let crates = self.fetch()?;

        let now = Utc::now();
        db.transaction(|t| {
            // Replace the existing list in the database
            t.execute("DELETE FROM crates WHERE list = ?1;", &[&Self::NAME])?;
            for krate in &crates {
                t.execute(
                    "INSERT INTO crates (crate, list, loaded_at) VALUES (?1, ?2, ?3);",
                    &[&::serde_json::to_string(krate)?, &Self::NAME, &now],
                ).chain_err(|| {
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

    fn get(db: &Database) -> Result<Vec<Crate>> {
        let crates_results = db.query(
            "SELECT crate FROM crates WHERE list = ?1 ORDER BY rowid;",
            &[&Self::NAME],
            |r| {
                let raw: String = r.get("crate");
                Ok(::serde_json::from_str(&raw)?)
            },
        )?;

        // Turns Vec<Result<Crate>> into Result<Vec<Crate>>
        crates_results.into_iter().collect()
    }
}

pub(crate) fn get_crates(
    select: CrateSelect,
    db: &Database,
    config: &Config,
) -> Result<Vec<Crate>> {
    let mut crates = Vec::new();

    match select {
        CrateSelect::Full => {
            crates.append(&mut RegistryList::get(db)?);
            crates.append(&mut GitHubList::get(db)?);
        }
        CrateSelect::Demo => {
            let mut demo_registry = config.demo_crates().crates.iter().collect::<HashSet<_>>();
            let mut demo_github = config
                .demo_crates()
                .github_repos
                .iter()
                .collect::<HashSet<_>>();

            let mut all_crates = Vec::new();
            all_crates.append(&mut RegistryList::get(db)?);
            all_crates.append(&mut GitHubList::get(db)?);

            for krate in all_crates.drain(..) {
                let add = match krate {
                    Crate::Registry(RegistryCrate { ref name, .. }) => demo_registry.remove(name),
                    Crate::GitHub(ref repo) => demo_github.remove(&repo.slug()),
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
        }
        CrateSelect::SmallRandom => {
            crates.append(&mut RegistryList::get(db)?);
            crates.append(&mut GitHubList::get(db)?);

            let mut rng = thread_rng();
            rng.shuffle(&mut crates);
            crates.truncate(SMALL_RANDOM_COUNT);
        }
        CrateSelect::Top100 => {
            crates.append(&mut RegistryList::get(db)?);
            crates.truncate(100);
        }
    }

    crates.sort();
    Ok(crates)
}

#[cfg(test)]
pub(crate) fn setup_test_lists(db: &Database, config: &Config) -> Result<()> {
    ::actions::UpdateLists::default().apply(db, config)
}
