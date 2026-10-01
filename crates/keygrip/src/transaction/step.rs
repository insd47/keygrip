use crate::Result;
use aws_sdk_dynamodb::types::TransactWriteItem;
use aws_sdk_dynamodb::Client;

/// One write compiled into a transaction item, with the client of the
/// [`Entity`](crate::Entity) it came from.
///
/// Converted from a [`Put`](crate::Put), [`Update`](crate::Update), or
/// [`Delete`](crate::Delete); a write that fails to compile carries its error
/// to [`Transaction::run`](super::Transaction::run).
#[derive(Debug)]
pub struct Step {
    pub item: Result<TransactWriteItem>,
    pub client: Client,
}
