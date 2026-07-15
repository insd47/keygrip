use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

/// Opaque pagination cursor, as returned by DynamoDB (`LastEvaluatedKey`).
///
/// Pass it back to [`Query::page`](crate::Query::page) to resume; `None`
/// means the result set is exhausted.
pub type Cursor = HashMap<String, AttributeValue>;

/// One page of query results plus the cursor to fetch the next one.
#[derive(Debug, Clone)]
pub struct Page<E> {
    pub items: Vec<E>,
    pub cursor: Option<Cursor>,
}

/// Sort-key constraint of a [`Query`](crate::Query).
pub enum Sort {
    Prefix(String),
    Equal(String),
}
