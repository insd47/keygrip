//! serde ↔ DynamoDB item conversions with this crate's error mapping, for
//! extension code that reads or writes raw items.

use crate::{Error, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;

/// Deserializes a response's item list, tolerating its absence.
pub fn page<E: DeserializeOwned>(
    items: Option<Vec<HashMap<String, AttributeValue>>>,
) -> Result<Vec<E>> {
    items.unwrap_or_default().into_iter().map(from).collect()
}

/// Deserializes one DynamoDB item.
pub fn from<T: DeserializeOwned>(item: HashMap<String, AttributeValue>) -> Result<T> {
    serde_dynamo::from_item(item).map_err(unavailable)
}

/// Deserializes an optional DynamoDB item.
pub fn option<T: DeserializeOwned>(
    item: Option<HashMap<String, AttributeValue>>,
) -> Result<Option<T>> {
    item.map(from).transpose()
}

/// Serializes a value into a DynamoDB item.
pub fn to<T: Serialize>(value: &T) -> Result<HashMap<String, AttributeValue>> {
    serde_dynamo::to_item(value).map_err(unavailable)
}

/// Serializes a value into a single attribute value.
pub fn value<T: Serialize>(value: T) -> Result<AttributeValue> {
    serde_dynamo::to_attribute_value(value).map_err(unavailable)
}

fn unavailable(error: serde_dynamo::Error) -> Error {
    Error::Unavailable(error.to_string())
}
