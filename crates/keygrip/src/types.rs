use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

/// Pagination cursor: DynamoDB's `LastEvaluatedKey`, the primary key
/// attributes of the last item read.
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
