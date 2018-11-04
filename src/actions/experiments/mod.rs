mod create;
mod delete;
mod edit;

pub use self::create::CreateExperiment;
pub use self::delete::DeleteExperiment;
pub use self::edit::EditExperiment;

#[derive(Debug, Fail)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum ExperimentError {
    #[fail(display = "experiment '{}' not found", _0)]
    NotFound(String),
    #[fail(display = "experiment '{}' already exists", _0)]
    AlreadyExists(String),
    #[fail(display = "duplicate toolchains provided")]
    DuplicateToolchains,
    #[fail(display = "it's only possible to edit queued experiments")]
    CanOnlyEditQueuedExperiments,
}
