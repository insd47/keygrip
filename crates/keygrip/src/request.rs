use crate::Error;
use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};

pub fn conditional<E, R>(error: &SdkError<E, R>) -> bool
where
    E: ProvideErrorMetadata,
{
    error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code)
        == Some("ConditionalCheckFailedException")
}

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

pub fn unavailable(error: impl std::fmt::Display) -> Error {
    Error::Unavailable(error.to_string())
}
