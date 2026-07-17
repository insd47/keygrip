//! Single-item conditional writes, for extension code implementing invariants
//! without dropping to raw SDK requests.
//!
//! [`occ`](crate::occ) retries a read-modify-write loop, this module performs
//! one conditional write, and [`transaction`](crate::transaction) commits
//! writes to several items atomically.
//!
//! Condition rejection is a value, not an error: [`run`](Update::run) returns
//! `false`, and domain code decides whether that means a retry, a fallback
//! read, or a conflict. A write and its condition must bind distinct
//! placeholders:
//!
//! ```no_run
//! use keygrip::expression::Expression;
//! use keygrip::{Entity, Result, Schema};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize, Schema)]
//! #[entity(pk(id))]
//! struct SubmissionTable {
//!     id: String,
//!     score: i64,
//! }
//!
//! async fn improve(submissions: &Entity<SubmissionTable>, id: &str, score: i64) -> Result<bool> {
//!     submissions
//!         .update(
//!             id,
//!             Expression::new("SET score = :score").number(":score", score),
//!         )
//!         .when(
//!             Expression::new("attribute_not_exists(score) OR score < :floor")
//!                 .number(":floor", score),
//!         )
//!         .run()
//!         .await
//! }
//! ```

use crate::binding::{Bindings, BoundExpression};
use crate::expression::Expression;
use crate::key::document_key;
use crate::{item, request, Entity, Error, Result, Schema};
use aws_sdk_dynamodb::operation::put_item::builders::PutItemFluentBuilder;
use aws_sdk_dynamodb::operation::update_item::builders::UpdateItemFluentBuilder;
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

impl<E: Schema> Entity<E> {
    /// Starts an update of the item at `primary` with `expression`.
    pub fn update<'a>(
        &self,
        primary: impl Into<E::Key<'a>>,
        expression: Expression,
    ) -> Update<'_, E>
    where
        E: 'a,
    {
        Update {
            entity: self,
            key: document_key(E::parts(primary)),
            expression,
            condition: Condition::default(),
        }
    }

    /// Starts a conditional write of the whole `value`.
    pub fn store<'a>(&'a self, value: &'a E) -> Store<'a, E> {
        Store {
            entity: self,
            value,
            condition: Condition::default(),
        }
    }
}

/// A conditional update of one item.
pub struct Update<'a, E: Schema> {
    entity: &'a Entity<E>,
    key: HashMap<String, AttributeValue>,
    expression: Expression,
    condition: Condition,
}

impl<E: Schema> Update<'_, E> {
    /// Attaches the condition that must hold for the update to apply.
    ///
    /// At most one condition may be attached; a second condition fails at
    /// [`run`](Self::run).
    pub fn when(mut self, condition: Expression) -> Self {
        self.condition.attach(condition);
        self
    }

    /// Applies the update, returning `false` when its condition is rejected.
    pub async fn run(self) -> Result<bool> {
        let request = self.request()?;

        match request.send().await {
            Ok(_) => Ok(true),
            Err(error) if request::conditional(&error) => Ok(false),
            Err(error) => Err(request::unavailable(error)),
        }
    }

    fn request(self) -> Result<UpdateItemFluentBuilder> {
        let condition = self.condition.ready()?;

        if let Some(condition) = &condition {
            let collision = self.expression.bindings().collision(condition.bindings());

            if let Some(collision) = collision {
                return Err(invalid(collision));
            }
        }

        let Bindings {
            statement,
            mut names,
            mut values,
        } = self.expression.into_bindings();
        let mut request = self
            .entity
            .client()
            .update_item()
            .table_name(self.entity.name())
            .set_key(Some(self.key))
            .update_expression(statement);

        if let Some(condition) = condition {
            let condition = condition.into_bindings();
            names.extend(condition.names);
            values.extend(condition.values);
            request = request.condition_expression(condition.statement);
        }

        Ok(request
            .set_expression_attribute_names(present(names))
            .set_expression_attribute_values(present(values)))
    }
}

/// A conditional write of one complete item.
pub struct Store<'a, E: Schema> {
    entity: &'a Entity<E>,
    value: &'a E,
    condition: Condition,
}

impl<E: Schema> Store<'_, E> {
    /// Attaches the condition that must hold for the item to be stored.
    ///
    /// At most one condition may be attached; a second condition fails at
    /// [`run`](Self::run).
    pub fn when(mut self, condition: Expression) -> Self {
        self.condition.attach(condition);
        self
    }

    /// Stores the item, returning `false` when its condition is rejected.
    pub async fn run(self) -> Result<bool> {
        let request = self.request()?;

        match request.send().await {
            Ok(_) => Ok(true),
            Err(error) if request::conditional(&error) => Ok(false),
            Err(error) => Err(request::unavailable(error)),
        }
    }

    fn request(self) -> Result<PutItemFluentBuilder> {
        let condition = self.condition.ready()?;
        let mut document = item::to(self.value)?;
        document.extend(document_key(E::parts(self.value.primary())));
        let mut request = self
            .entity
            .client()
            .put_item()
            .table_name(self.entity.name())
            .set_item(Some(document));

        if let Some(condition) = condition {
            let Bindings {
                statement,
                names,
                values,
            } = condition.into_bindings();
            request = request
                .condition_expression(statement)
                .set_expression_attribute_names(present(names))
                .set_expression_attribute_values(present(values));
        }

        Ok(request)
    }
}

#[derive(Default)]
struct Condition {
    expression: Option<Expression>,
    problem: Option<String>,
}

impl Condition {
    fn attach(&mut self, expression: Expression) {
        if self.expression.is_some() {
            self.problem = Some("a conditional write cannot have more than one condition".into());

            return;
        }

        self.expression = Some(expression);
    }

    fn ready(self) -> Result<Option<Expression>> {
        match self.problem {
            Some(problem) => Err(invalid(problem)),
            None => Ok(self.expression),
        }
    }
}

fn invalid(detail: impl Into<String>) -> Error {
    Error::Unavailable(format!("invalid conditional write: {}", detail.into()))
}

fn present<K, V>(values: HashMap<K, V>) -> Option<HashMap<K, V>> {
    (!values.is_empty()).then_some(values)
}

#[cfg(test)]
mod tests {
    use super::Update;
    use crate::expression::Expression;
    use crate::{attr, Entity};
    use aws_sdk_dynamodb::config::BehaviorVersion;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_dynamodb::{Client, Config};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(scope, owner), sk(kind, id))]
    #[serde(rename_all = "camelCase")]
    struct RecordTable {
        scope: String,
        owner: String,
        kind: String,
        id: String,
        active: bool,
    }

    #[test]
    fn attaches_conditions_and_arbitrary_values() {
        let records = entity();
        let request = records
            .update(
                ("contest", "user", "submission", "one"),
                Expression::new("SET active = :active, tags = :tags")
                    .boolean(":active", true)
                    .value(":tags", attr::list(vec![attr::s("tag")])),
            )
            .when(Expression::new("attribute_exists(pk)"))
            .request()
            .unwrap();
        let input = request.as_input();
        let condition = input.get_condition_expression().as_deref();
        let update = input.get_update_expression().as_deref();

        assert_eq!(condition, Some("attribute_exists(pk)"));
        assert_eq!(update, Some("SET active = :active, tags = :tags"));
        assert!(matches!(
            input
                .get_expression_attribute_values()
                .as_ref()
                .and_then(|values| values.get(":active")),
            Some(AttributeValue::Bool(true))
        ));
        assert!(matches!(
            input
                .get_expression_attribute_values()
                .as_ref()
                .and_then(|values| values.get(":tags")),
            Some(AttributeValue::L(values))
                if matches!(&values[0], AttributeValue::S(value) if value == "tag")
        ));
    }

    #[tokio::test]
    async fn rejects_a_second_condition_at_run_time() {
        let records = entity();
        let error = update(&records)
            .when(Expression::new("attribute_exists(pk)"))
            .when(Expression::new("attribute_exists(sk)"))
            .run()
            .await
            .unwrap_err();

        assert!(error.to_string().contains("more than one condition"));
    }

    #[tokio::test]
    async fn rejects_update_condition_placeholder_collisions_at_run_time() {
        let records = entity();
        let error = records
            .update(
                ("contest", "user", "submission", "one"),
                Expression::new("SET #state = :next")
                    .name("#state", "state")
                    .string(":next", "next"),
            )
            .when(Expression::new("#state = :next").name("#state", "previous"))
            .run()
            .await
            .unwrap_err();

        assert!(error.to_string().contains("placeholder"));
    }

    #[test]
    fn composes_composite_update_keys() {
        let records = entity();
        let request = update(&records).request().unwrap();
        let key = request.as_input().get_key().as_ref().unwrap();

        assert_eq!(key["pk"].as_s().unwrap(), "contest#user");
        assert_eq!(key["sk"].as_s().unwrap(), "submission#one");
    }

    #[test]
    fn serializes_store_values_with_composed_keys() {
        let records = entity();
        let record = RecordTable {
            scope: "contest".into(),
            owner: "user".into(),
            kind: "submission".into(),
            id: "one".into(),
            active: true,
        };
        let request = records
            .store(&record)
            .when(Expression::new("attribute_not_exists(pk)"))
            .request()
            .unwrap();
        let input = request.as_input();
        let item = input.get_item().as_ref().unwrap();
        let condition = input.get_condition_expression().as_deref();

        assert_eq!(condition, Some("attribute_not_exists(pk)"));
        assert_eq!(item["pk"].as_s().unwrap(), "contest#user");
        assert_eq!(item["sk"].as_s().unwrap(), "submission#one");
        assert_eq!(item["scope"].as_s().unwrap(), "contest");
        assert!(matches!(item["active"], AttributeValue::Bool(true)));
    }

    fn update(records: &Entity<RecordTable>) -> Update<'_, RecordTable> {
        records.update(
            ("contest", "user", "submission", "one"),
            Expression::new("SET active = :active").boolean(":active", true),
        )
    }

    fn entity() -> Entity<RecordTable> {
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .build();
        let client = Client::from_conf(config);

        Entity::new(&client, "Records")
    }
}
