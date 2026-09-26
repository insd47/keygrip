use super::{applied, clause, Clause, Condition, Pending, Slot};
use crate::{Entity, Result, Schema};
use aws_sdk_dynamodb::operation::delete_item::builders::DeleteItemFluentBuilder;
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;
use std::future::{Future, IntoFuture};

/// A removal of the item at one key.
///
/// Built by [`Entity::delete`]. Awaiting it resolves to `true` when the
/// delete applied — including when no item existed — and `false` when its
/// [`Condition`] was rejected.
#[must_use = "a write does nothing until it is awaited"]
pub struct Delete<'a, E: Schema> {
    entity: &'a Entity<E>,
    key: HashMap<String, AttributeValue>,
    condition: Slot,
}

pub(crate) struct Compiled {
    pub(crate) table: String,
    pub(crate) key: HashMap<String, AttributeValue>,
    pub(crate) clause: Clause,
}

impl<'a, E: Schema> Delete<'a, E> {
    pub(crate) fn new(entity: &'a Entity<E>, key: HashMap<String, AttributeValue>) -> Self {
        Self {
            entity,
            key,
            condition: Slot::default(),
        }
    }

    /// Attaches the condition that must hold for the item to be removed.
    ///
    /// At most one condition may be attached; a second one fails with
    /// [`Error::Invalid`](crate::Error::Invalid) when the write runs.
    pub fn when(mut self, condition: impl Into<Condition>) -> Self {
        self.condition.attach(condition.into());
        self
    }

    /// Removes the item, resolving to `false` when its condition is rejected.
    ///
    /// Awaiting the `Delete` directly does the same.
    pub fn run(self) -> impl Future<Output = Result<bool>> + Send + 'a {
        let request = self.request();

        async move { Ok(applied(request?.send().await)?.is_some()) }
    }

    pub(crate) fn compile(self) -> Result<Compiled> {
        let (_, clause) = clause(None, self.condition.resolve::<E>()?)?;

        Ok(Compiled {
            table: self.entity.name().into(),
            key: self.key,
            clause,
        })
    }

    fn request(self) -> Result<DeleteItemFluentBuilder> {
        let entity = self.entity;
        let Compiled { table, key, clause } = self.compile()?;

        Ok(entity
            .client()
            .delete_item()
            .table_name(table)
            .set_key(Some(key))
            .set_condition_expression(clause.condition)
            .set_expression_attribute_names(clause.names)
            .set_expression_attribute_values(clause.values))
    }
}

impl<'a, E: Schema> IntoFuture for Delete<'a, E> {
    type Output = Result<bool>;
    type IntoFuture = Pending<'a, bool>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}
