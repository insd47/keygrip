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
/// use keygrip::Expression;
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
    pub fn value<T: Serialize + ?Sized>(mut self, placeholder: impl Into<String>, value: &T) -> Self {
        let placeholder = placeholder.into();

        match serde_dynamo::to_attribute_value(value) {
            Ok(value) => {
                self.bindings.values.insert(placeholder, value);
            }
            Err(error) => {
                self.problem
                    .get_or_insert_with(|| format!("value placeholder {placeholder} does not serialize: {error}"));
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
