mod create;
mod delete;
mod edit;

pub use self::create::CreateExperiment;
pub use self::delete::DeleteExperiment;
pub use self::edit::EditExperiment;

#[derive(Debug, thiserror::Error)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum ExperimentError {
    #[error("experiment '{0}' not found")]
    NotFound(String),
    #[error("experiment '{0}' already exists")]
    AlreadyExists(String),
    #[error("duplicate toolchains provided")]
    DuplicateToolchains,
    #[error("it's only possible to edit queued experiments")]
    CanOnlyEditQueuedExperiments,
}
