use thiserror::Error;
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("Found no username in {0}")]
pub struct AppError(pub String);
