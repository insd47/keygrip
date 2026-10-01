use super::{applied, clause, invalid, Clause, Condition, Pending, Slot};
use crate::key::document_key;
use crate::transaction::Step;
use crate::{item, Entity, Result, Schema};
use aws_sdk_dynamodb::operation::put_item::builders::PutItemFluentBuilder;
use aws_sdk_dynamodb::types::{self, AttributeValue, TransactWriteItem};
use std::collections::HashMap;
use std::future::{Future, IntoFuture};

/// A write of one complete item, replacing any item at the same key.
///
/// Built by [`Entity::put`]. Awaiting it resolves to `true` when the item was
/// written and `false` when its [`Condition`] was rejected:
///
/// ```no_run
/// # use keygrip::{Condition, Entity, Result, Schema};
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Serialize, Deserialize, Schema)]
/// # #[entity(pk(id))]
/// # struct UserTable { id: String }
/// async fn register(users: &Entity<UserTable>, user: &UserTable) -> Result<bool> {
///     users.put(user).when(Condition::absent()).await
/// }
/// ```
#[must_use = "a write does nothing until it is awaited"]
pub struct Put<'a, E: Schema> {
    entity: &'a Entity<E>,
    value: &'a E,
    condition: Slot,
}

struct Compiled {
    table: String,
    item: HashMap<String, AttributeValue>,
    clause: Clause,
}

impl<'a, E: Schema> Put<'a, E> {
    pub(crate) fn new(entity: &'a Entity<E>, value: &'a E) -> Self {
        Self {
            entity,
            value,
            condition: Slot::default(),
        }
    }

    /// Attaches the condition that must hold for the item to be written.
    ///
    /// At most one condition may be attached; a second one fails with
    /// [`Error::Invalid`](crate::Error::Invalid) when the write runs.
    pub fn when(mut self, condition: impl Into<Condition>) -> Self {
        self.condition.attach(condition.into());
        self
    }

    /// Writes the item, resolving to `false` when its condition is rejected.
    ///
    /// Awaiting the `Put` directly does the same.
    pub fn run(self) -> impl Future<Output = Result<bool>> + Send + 'a {
        let request = self.request();

        async move { Ok(applied(request?.send().await)?.is_some()) }
    }

    fn compile(self) -> Result<Compiled> {
        let (_, clause) = clause(None, self.condition.resolve::<E>()?)?;
        let mut item = item::to(self.value)?;
        item.extend(document_key(E::parts(self.value.primary())));

        Ok(Compiled {
            table: self.entity.name().into(),
            item,
            clause,
        })
    }

    fn request(self) -> Result<PutItemFluentBuilder> {
        let entity = self.entity;
        let Compiled { table, item, clause } = self.compile()?;

        Ok(entity
            .client()
            .put_item()
            .table_name(table)
            .set_item(Some(item))
            .set_condition_expression(clause.condition)
            .set_expression_attribute_names(clause.names)
            .set_expression_attribute_values(clause.values))
    }
}

impl<'a, E: Schema> IntoFuture for Put<'a, E> {
    type Output = Result<bool>;
    type IntoFuture = Pending<'a, bool>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

impl<E: Schema> From<Put<'_, E>> for Step {
    fn from(write: Put<'_, E>) -> Self {
        let client = write.entity.client().clone();
        let item = write.compile().and_then(|Compiled { table, item, clause }| {
            let put = types::Put::builder()
                .table_name(table)
                .set_item(Some(item))
                .set_condition_expression(clause.condition)
                .set_expression_attribute_names(clause.names)
                .set_expression_attribute_values(clause.values)
                .build()
                .map_err(|error| invalid(error.to_string()))?;

            Ok(TransactWriteItem::builder().put(put).build())
        });

        Step { item, client }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Condition, Entity};
    use aws_sdk_dynamodb::config::BehaviorVersion;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_dynamodb::{Client, Config};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(scope, owner), sk(kind, id))]
    struct RecordTable {
        scope: String,
        owner: String,
        kind: String,
        id: String,
        active: bool,
    }

    #[test]
    fn serializes_put_values_with_composed_keys() {
        let records = entity();
        let record = RecordTable {
            scope: "contest".into(),
            owner: "user".into(),
            kind: "submission".into(),
            id: "one".into(),
            active: true,
        };
        let put = records.put(&record).when(Condition::absent()).compile().unwrap();
        let names = put.clause.names.unwrap();

        assert_eq!(put.table, "Records");
        assert_eq!(
            put.clause.condition.as_deref(),
            Some("attribute_not_exists(#keygripKey)")
        );
        assert_eq!(names["#keygripKey"], "pk");
        assert_eq!(put.item["pk"].as_s().unwrap(), "contest#user");
        assert_eq!(put.item["sk"].as_s().unwrap(), "submission#one");
        assert_eq!(put.item["scope"].as_s().unwrap(), "contest");
        assert!(matches!(put.item["active"], AttributeValue::Bool(true)));
    }

    fn entity() -> Entity<RecordTable> {
        let config = Config::builder().behavior_version(BehaviorVersion::latest()).build();

        Entity::new(&Client::from_conf(config), "Records")
    }
}
