//! Command pattern for mutating experiment state — creating, editing, and deleting
//! experiments and updating their crate lists.

mod experiments;
mod lists;

pub use self::experiments::*;
pub use self::lists::*;

use crate::config::Config;
use crate::db::Database;
use crate::prelude::*;

/// A mutation that can be applied to experiment state.
pub trait Action {
    fn apply(self, ctx: &ActionsCtx) -> Fallible<()>;
}

/// Shared context (database + config) passed to every [`Action`].
pub struct ActionsCtx<'ctx> {
    db: &'ctx Database,
    config: &'ctx Config,
}

impl<'ctx> ActionsCtx<'ctx> {
    pub fn new(db: &'ctx Database, config: &'ctx Config) -> Self {
        ActionsCtx { db, config }
    }
}
