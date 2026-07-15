use aws_sdk_dynamodb::types::AttributeValue;

pub fn boolean(value: bool) -> AttributeValue {
    AttributeValue::Bool(value)
}

pub fn list(values: Vec<AttributeValue>) -> AttributeValue {
    AttributeValue::L(values)
}

pub fn n(value: impl ToString) -> AttributeValue {
    AttributeValue::N(value.to_string())
}

pub fn s(value: impl Into<String>) -> AttributeValue {
    AttributeValue::S(value.into())
}
