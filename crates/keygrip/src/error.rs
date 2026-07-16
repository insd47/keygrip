/// Failure modes of DynamoDB operations, neutral to any application.
///
/// Applications typically wrap this in their own error type via `From` and
/// add domain-specific variants there.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A conditional write was rejected (for example, [`Entity::create`] hit
    /// an existing item).
    ///
    /// [`Entity::create`]: crate::Entity::create
    #[error("{0}")]
    Conflict(&'static str),
    /// The requested item does not exist.
    #[error("{0}")]
    NotFound(String),
    /// DynamoDB or (de)serialization failed; the operation may succeed on retry.
    #[error("database unavailable: {0}")]
    Unavailable(String),
}

/// Convenience alias for results produced by this crate.
pub type Result<T> = std::result::Result<T, Error>;
