use crate::{attr, Parts};
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

pub fn document_key(parts: Parts) -> HashMap<String, AttributeValue> {
    let mut key = HashMap::from([(parts.partition.0.into(), attr::s(parts.partition.1))]);

    if let Some((name, value)) = parts.sort {
        key.insert(name.into(), attr::s(value));
    }

    key
}
