use super::{applied, clause, invalid, Clause, Condition, Pending, Slot};
use crate::transaction::Step;
use crate::{Entity, Result, Schema};
use aws_sdk_dynamodb::operation::delete_item::builders::DeleteItemFluentBuilder;
use aws_sdk_dynamodb::types::{self, AttributeValue, TransactWriteItem};
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

struct Compiled {
    table: String,
    key: HashMap<String, AttributeValue>,
    clause: Clause,
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

    fn compile(self) -> Result<Compiled> {
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

impl<E: Schema> From<Delete<'_, E>> for Step {
    fn from(write: Delete<'_, E>) -> Self {
        let client = write.entity.client().clone();
        let item = write.compile().and_then(|Compiled { table, key, clause }| {
            let delete = types::Delete::builder()
                .table_name(table)
                .set_key(Some(key))
                .set_condition_expression(clause.condition)
                .set_expression_attribute_names(clause.names)
                .set_expression_attribute_values(clause.values)
                .build()
                .map_err(|error| invalid(error.to_string()))?;

            Ok(TransactWriteItem::builder().delete(delete).build())
        });

        Step { item, client }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Condition, Entity};
    use aws_sdk_dynamodb::config::BehaviorVersion;
    use aws_sdk_dynamodb::{Client, Config};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(user_id), sk(problem_id))]
    #[serde(rename_all = "camelCase")]
    struct SubmissionTable {
        user_id: String,
        problem_id: String,
    }

    #[test]
    fn resolves_existence_conditions_against_the_key_schema() {
        let submissions = entity();
        let delete = submissions
            .delete(("user", "problem"))
            .when(Condition::exists())
            .compile()
            .unwrap();

        assert_eq!(
            delete.clause.condition.as_deref(),
            Some("attribute_exists(#keygripKey)")
        );
        assert_eq!(delete.clause.names.unwrap()["#keygripKey"], "userId");
        assert_eq!(delete.key["problemId"].as_s().unwrap(), "problem");
    }

    fn entity() -> Entity<SubmissionTable> {
        let config = Config::builder().behavior_version(BehaviorVersion::latest()).build();

        Entity::new(&Client::from_conf(config), "Submissions")
    }
}
