mod experiments;
mod lists;

pub use self::experiments::*;
pub use self::lists::*;

use crate::config::Config;
use crate::db::Database;
use crate::prelude::*;

pub trait Action {
    fn apply(self, ctx: &ActionsCtx) -> Fallible<()>;
}

pub struct ActionsCtx<'ctx> {
    db: &'ctx Database,
    config: &'ctx Config,
}

impl<'ctx> ActionsCtx<'ctx> {
    pub fn new(db: &'ctx Database, config: &'ctx Config) -> Self {
        ActionsCtx { db, config }
    }
}
