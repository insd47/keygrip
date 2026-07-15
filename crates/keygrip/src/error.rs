#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Conflict(&'static str),
    #[error("{0}")]
    NotFound(String),
    #[error("database unavailable: {0}")]
    Unavailable(String),
}

pub type Result<T> = std::result::Result<T, Error>;
