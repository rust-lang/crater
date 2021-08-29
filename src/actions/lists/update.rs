use crate::actions::{Action, ActionsCtx};
use crate::crates::lists::{GitHubList, List, LocalList, RegistryList};
use crate::prelude::*;

pub struct UpdateLists {
    pub github: bool,
    pub registry: bool,
    pub local: bool,
}

impl Default for UpdateLists {
    fn default() -> Self {
        UpdateLists {
            github: true,
            registry: true,
            local: true,
        }
    }
}

impl Action for UpdateLists {
    fn apply(self, ctx: &ActionsCtx) -> Fallible<()> {
        if self.github {
            info!("updating GitHub repositories list");
            GitHubList::default().update(ctx.db)?;
        }

        if self.registry {
            info!("updating crates.io crates list");
            RegistryList.update(ctx.db)?;
        }

        if self.local {
            info!("updating local crates list");
            LocalList::default().update(ctx.db)?;
        }

        Ok(())
    }
}
