use thiserror::Error;
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("App run error,the error is {0}")]
pub struct AppError(pub String);
