use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Bindings {
    pub statement: String,
    pub names: HashMap<String, String>,
    pub values: HashMap<String, AttributeValue>,
}

impl Bindings {
    pub fn collision(&self, condition: &Self) -> Option<String> {
        if let Some(placeholder) = condition
            .names
            .keys()
            .find(|placeholder| self.names.contains_key(*placeholder))
        {
            return Some(format!(
                "expression name placeholder {placeholder} is bound by both the update and condition"
            ));
        }

        condition
            .values
            .keys()
            .find(|placeholder| self.values.contains_key(*placeholder))
            .map(|placeholder| {
                format!(
                    "expression value placeholder {placeholder} is bound by both the update and condition"
                )
            })
    }
}

pub trait BoundExpression {
    fn bindings(&self) -> &Bindings;
    fn into_bindings(self) -> Bindings;
}
