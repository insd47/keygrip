use crate::{Error, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;

pub fn page<E: DeserializeOwned>(
    items: Option<Vec<HashMap<String, AttributeValue>>>,
) -> Result<Vec<E>> {
    items.unwrap_or_default().into_iter().map(from).collect()
}

pub fn from<T: DeserializeOwned>(item: HashMap<String, AttributeValue>) -> Result<T> {
    serde_dynamo::from_item(item).map_err(unavailable)
}

pub fn option<T: DeserializeOwned>(
    item: Option<HashMap<String, AttributeValue>>,
) -> Result<Option<T>> {
    item.map(from).transpose()
}

pub fn to<T: Serialize>(value: &T) -> Result<HashMap<String, AttributeValue>> {
    serde_dynamo::to_item(value).map_err(unavailable)
}

pub fn value<T: Serialize>(value: T) -> Result<AttributeValue> {
    serde_dynamo::to_attribute_value(value).map_err(unavailable)
}

fn unavailable(error: serde_dynamo::Error) -> Error {
    Error::Unavailable(error.to_string())
}
