//! SDK error mapping helpers, for extension code that sends its own requests.

use crate::Error;
use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};

/// Whether the error is a rejected condition expression
/// (`ConditionalCheckFailedException`).
pub fn conditional<E, R>(error: &SdkError<E, R>) -> bool
where
    E: ProvideErrorMetadata,
{
    error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code)
        == Some("ConditionalCheckFailedException")
}

/// Maps a rejected condition to [`Error::Conflict`] with the given detail,
/// and anything else to [`Error::Unavailable`].
pub fn conflict<E, R>(error: SdkError<E, R>, detail: &'static str) -> Error
where
    E: ProvideErrorMetadata,
{
    if conditional(&error) {
        Error::Conflict(detail)
    } else {
        unavailable(error)
    }
}

/// Maps any displayable error to [`Error::Unavailable`].
pub fn unavailable(error: impl std::fmt::Display) -> Error {
    Error::Unavailable(error.to_string())
}
