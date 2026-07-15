//! Shorthand constructors for [`AttributeValue`], for extension code that
//! builds its own expressions.

use aws_sdk_dynamodb::types::AttributeValue;

/// A boolean attribute value (`BOOL`).
pub fn boolean(value: bool) -> AttributeValue {
    AttributeValue::Bool(value)
}

/// A list attribute value (`L`).
pub fn list(values: Vec<AttributeValue>) -> AttributeValue {
    AttributeValue::L(values)
}

/// A number attribute value (`N`).
pub fn n(value: impl ToString) -> AttributeValue {
    AttributeValue::N(value.to_string())
}

/// A string attribute value (`S`).
pub fn s(value: impl Into<String>) -> AttributeValue {
    AttributeValue::S(value.into())
}
