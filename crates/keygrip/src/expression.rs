//! DynamoDB expressions with placeholder bindings, shared by
//! [`Update`](crate::Update), [`Condition`](crate::Condition), and
//! [`Transaction`](crate::Transaction) steps.

use crate::binding::Bindings;
use crate::{Error, Result};
use serde::Serialize;
use std::collections::HashMap;

/// A DynamoDB expression together with its placeholder bindings.
///
/// Values are bound through serde, so anything the entity itself stores —
/// strings, numbers, enums, lists, nested structs — binds directly:
///
/// ```
/// use keygrip::expression::Expression;
///
/// let review = Expression::new("SET #state = :state, tags = :tags")
///     .name("#state", "state")
///     .value(":state", "REVIEWED")
///     .value(":tags", &vec!["late", "manual"]);
/// ```
#[derive(Debug, Clone)]
pub struct Expression {
    bindings: Bindings,
    problem: Option<String>,
}

impl Expression {
    /// Creates an expression with no bindings.
    pub fn new(statement: impl Into<String>) -> Self {
        Self {
            bindings: Bindings {
                statement: statement.into(),
                names: HashMap::new(),
                values: HashMap::new(),
            },
            problem: None,
        }
    }

    /// Binds an attribute name placeholder (`#…`).
    pub fn name(mut self, placeholder: impl Into<String>, name: impl Into<String>) -> Self {
        self.bindings.names.insert(placeholder.into(), name.into());
        self
    }

    /// Binds a value placeholder (`:…`) to the serde representation of
    /// `value`.
    ///
    /// A value that fails to serialize surfaces as [`Error::Invalid`] when
    /// the write runs.
    pub fn value<T: Serialize + ?Sized>(
        mut self,
        placeholder: impl Into<String>,
        value: &T,
    ) -> Self {
        let placeholder = placeholder.into();

        match serde_dynamo::to_attribute_value(value) {
            Ok(value) => {
                self.bindings.values.insert(placeholder, value);
            }
            Err(error) => {
                self.problem.get_or_insert_with(|| {
                    format!("value placeholder {placeholder} does not serialize: {error}")
                });
            }
        }

        self
    }

    pub(crate) fn compile(self) -> Result<Bindings> {
        match self.problem {
            Some(problem) => Err(Error::Invalid(problem)),
            None => Ok(self.bindings),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Expression;
    use crate::Error;
    use aws_sdk_dynamodb::types::AttributeValue;
    use serde::ser::Error as _;
    use serde::{Serialize, Serializer};

    struct Unserializable;

    impl Serialize for Unserializable {
        fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(S::Error::custom("refused"))
        }
    }

    #[test]
    fn binds_serde_values() {
        let bindings = Expression::new("SET score = :score, tags = :tags")
            .value(":score", &3)
            .value(":tags", &["a"])
            .compile()
            .unwrap();

        assert!(matches!(&bindings.values[":score"], AttributeValue::N(value) if value == "3"));
        assert!(
            matches!(&bindings.values[":tags"], AttributeValue::L(values) if values.len() == 1)
        );
    }

    #[test]
    fn defers_serialization_failures_as_invalid() {
        let error = Expression::new("SET value = :value")
            .value(":value", &Unserializable)
            .compile()
            .unwrap_err();

        assert!(matches!(error, Error::Invalid(detail) if detail.contains(":value")));
    }
}
