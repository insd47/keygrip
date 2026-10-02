use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Bindings {
    pub statement: String,
    pub names: HashMap<String, String>,
    pub values: HashMap<String, AttributeValue>,
}

impl Bindings {
    /// Describes the first placeholder that the update and `condition` bind to
    /// different targets.
    ///
    /// DynamoDB shares one placeholder map between both expressions, so a
    /// placeholder bound to the same target on both sides is one binding;
    /// different targets would silently overwrite each other.
    pub fn conflict(&self, condition: &Self) -> Option<String> {
        let name = condition.names.iter().find_map(|(placeholder, name)| {
            let update = self.names.get(placeholder).filter(|update| *update != name)?;

            Some(format!(
                "expression name placeholder {placeholder} is bound to {update:?} by the update and to {name:?} by the condition"
            ))
        });

        name.or_else(|| {
            condition.values.iter().find_map(|(placeholder, value)| {
                let update = self.values.get(placeholder).filter(|update| *update != value)?;

                Some(format!(
                    "expression value placeholder {placeholder} is bound to {update:?} by the update and to {value:?} by the condition"
                ))
            })
        })
    }
}
