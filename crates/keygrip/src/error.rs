/// Failure modes of DynamoDB operations, neutral to any application.
///
/// Applications typically wrap this in their own error type via `From` and
/// add domain-specific variants there. New variants may be added in minor
/// releases, so a `match` needs a wildcard arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The requested item does not exist.
    #[error("{0}")]
    NotFound(String),
    /// The operation cannot succeed as written — a malformed or conflicting
    /// expression, a value or stored item that does not fit its serde model,
    /// or an unsupported combination of options. Retrying does not help.
    #[error("invalid operation: {0}")]
    Invalid(String),
    /// DynamoDB could not be reached or rejected the request for a transient
    /// reason; the operation may succeed on retry.
    #[error("database unavailable: {0}")]
    Unavailable(String),
}

/// Convenience alias for results produced by this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
