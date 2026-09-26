use crate::Parts;
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

/// Renders resolved key parts into the attribute map DynamoDB expects.
pub fn document_key(parts: Parts) -> HashMap<String, AttributeValue> {
    let mut key = HashMap::from([(
        parts.partition.0.into(),
        AttributeValue::S(parts.partition.1),
    )]);

    if let Some((name, value)) = parts.sort {
        key.insert(name.into(), AttributeValue::S(value));
    }

    key
}
