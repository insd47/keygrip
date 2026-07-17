//! DynamoDB expression bindings, shared by the conditional-write
//! ([`write`](crate::write)) and atomic-write ([`transaction`](crate::transaction))
//! builders.

use crate::attr;
use crate::binding::{Bindings, BoundExpression};
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

/// A DynamoDB expression together with its placeholder bindings.
#[derive(Debug, Clone)]
pub struct Expression {
    bindings: Bindings,
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
        }
    }

    /// Binds an attribute name placeholder (`#…`).
    pub fn name(mut self, placeholder: impl Into<String>, name: impl Into<String>) -> Self {
        self.bindings.names.insert(placeholder.into(), name.into());
        self
    }

    /// Binds a string value placeholder (`:…`).
    pub fn string(mut self, placeholder: impl Into<String>, value: impl Into<String>) -> Self {
        let placeholder = placeholder.into();
        self.bindings.values.insert(placeholder, attr::s(value));
        self
    }

    /// Binds a number value placeholder (`:…`).
    pub fn number(mut self, placeholder: impl Into<String>, value: impl ToString) -> Self {
        let placeholder = placeholder.into();
        self.bindings.values.insert(placeholder, attr::n(value));
        self
    }

    /// Binds a boolean value placeholder (`:…`).
    pub fn boolean(mut self, placeholder: impl Into<String>, value: bool) -> Self {
        let placeholder = placeholder.into();
        self.bindings
            .values
            .insert(placeholder, attr::boolean(value));
        self
    }

    /// Binds an arbitrary attribute value placeholder (`:…`).
    pub fn value(mut self, placeholder: impl Into<String>, value: AttributeValue) -> Self {
        self.bindings.values.insert(placeholder.into(), value);
        self
    }
}

impl BoundExpression for Expression {
    fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    fn into_bindings(self) -> Bindings {
        self.bindings
    }
}
