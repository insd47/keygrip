use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

pub type Cursor = HashMap<String, AttributeValue>;

#[derive(Debug, Clone)]
pub struct Page<E> {
    pub items: Vec<E>,
    pub cursor: Option<Cursor>,
}

pub enum Sort {
    Prefix(String),
    Equal(String),
}
