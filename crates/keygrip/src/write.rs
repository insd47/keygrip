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
//! [`Merge`] updates every serialized attribute but does not remove attributes
//! absent from the new value. Use it only when the field set is preserved
//! across writes; [`Store`] remains the full-replacement operation.
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
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
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

    /// Starts a conditional merge of every serialized attribute in `value`.
    ///
    /// Attributes absent from `value` are not removed. Use this only when the
    /// field set is preserved across writes.
    pub fn merge<'a>(&'a self, value: &'a E) -> Merge<'a, E> {
        Merge {
            entity: self,
            value,
            keep: Vec::new(),
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

/// A conditional merge of every serialized attribute in one item.
pub struct Merge<'a, E: Schema> {
    entity: &'a Entity<E>,
    value: &'a E,
    keep: Vec<String>,
    condition: Condition,
}

impl<E: Schema> Merge<'_, E> {
    /// Writes `attribute` only when the stored item does not already have it.
    ///
    /// Unknown serialized attribute names fail when the merge is compiled.
    pub fn keep(mut self, attribute: impl Into<String>) -> Self {
        self.keep.push(attribute.into());
        self
    }

    /// Attaches the condition that must hold for the merge to apply.
    ///
    /// At most one condition may be attached; a second condition fails at
    /// [`run`](Self::run) or [`fetch`](Self::fetch).
    pub fn when(mut self, condition: Expression) -> Self {
        self.condition.attach(condition);
        self
    }

    /// Applies the merge, returning `false` when its condition is rejected.
    pub async fn run(self) -> Result<bool> {
        let request = self.request()?;

        match request.send().await {
            Ok(_) => Ok(true),
            Err(error) if request::conditional(&error) => Ok(false),
            Err(error) => Err(request::unavailable(error)),
        }
    }

    /// Applies the merge and returns the stored item.
    ///
    /// Returns `None` when the condition is rejected.
    pub async fn fetch(self) -> Result<Option<E>> {
        let request = self.fetch_request()?;

        match request.send().await {
            Ok(response) => item::option(response.attributes),
            Err(error) if request::conditional(&error) => Ok(None),
            Err(error) => Err(request::unavailable(error)),
        }
    }

    fn fetch_request(self) -> Result<UpdateItemFluentBuilder> {
        Ok(self.request()?.return_values(ReturnValue::AllNew))
    }

    fn request(self) -> Result<UpdateItemFluentBuilder> {
        let condition = self.condition.ready()?;
        let key = document_key(E::parts(self.value.primary()));
        let mut document = item::to(self.value)?;

        for attribute in key.keys() {
            document.remove(attribute);
        }

        for attribute in &self.keep {
            if !document.contains_key(attribute) {
                return Err(invalid(format!("unknown merge keep attribute {attribute}")));
            }
        }

        if document.is_empty() {
            return Err(invalid("a merge must write at least one attribute"));
        }

        let mut attributes = document.into_iter().collect::<Vec<_>>();
        attributes.sort_unstable_by(|left, right| left.0.cmp(&right.0));

        let mut assignments = Vec::with_capacity(attributes.len());
        let mut names = HashMap::with_capacity(attributes.len());
        let mut values = HashMap::with_capacity(attributes.len());

        for (index, (attribute, value)) in attributes.into_iter().enumerate() {
            let name = format!("#m{index}");
            let value_name = format!(":m{index}");
            let assignment = if self.keep.contains(&attribute) {
                format!("{name} = if_not_exists({name}, {value_name})")
            } else {
                format!("{name} = {value_name}")
            };

            assignments.push(assignment);
            names.insert(name, attribute);
            values.insert(value_name, value);
        }

        let merge = Bindings {
            statement: format!("SET {}", assignments.join(", ")),
            names,
            values,
        };

        if let Some(condition) = &condition {
            if let Some(collision) = merge.collision(condition.bindings()) {
                return Err(invalid(collision));
            }
        }

        let Bindings {
            statement,
            mut names,
            mut values,
        } = merge;
        let mut request = self
            .entity
            .client()
            .update_item()
            .table_name(self.entity.name())
            .set_key(Some(key))
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
    /// [`run`](Self::run) or [`fetch`](Self::fetch).
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

    /// Applies the update and returns the stored item.
    ///
    /// Returns `None` when the condition is rejected.
    pub async fn fetch(self) -> Result<Option<E>> {
        let request = self.fetch_request()?;

        match request.send().await {
            Ok(response) => item::option(response.attributes),
            Err(error) if request::conditional(&error) => Ok(None),
            Err(error) => Err(request::unavailable(error)),
        }
    }

    fn fetch_request(self) -> Result<UpdateItemFluentBuilder> {
        Ok(self.request()?.return_values(ReturnValue::AllNew))
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
    use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
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

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(user_id), sk(problem_id))]
    #[serde(rename_all = "camelCase")]
    struct SubmissionTable {
        user_id: String,
        problem_id: String,
        id: String,
        created_at: i64,
        #[serde(rename = "type")]
        kind: String,
        score: i64,
    }

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(id))]
    struct KeyOnlyTable {
        id: String,
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

    #[test]
    fn composes_sorted_merge_assignments() {
        let submissions = submission_entity();
        let submission = submission();
        let request = submissions.merge(&submission).request().unwrap();
        let input = request.as_input();
        let update = input.get_update_expression().as_deref();
        let names = input.get_expression_attribute_names().as_ref().unwrap();

        assert_eq!(
            update,
            Some("SET #m0 = :m0, #m1 = :m1, #m2 = :m2, #m3 = :m3")
        );
        assert_eq!(names["#m0"], "createdAt");
        assert_eq!(names["#m1"], "id");
        assert_eq!(names["#m2"], "score");
        assert_eq!(names["#m3"], "type");
    }

    #[test]
    fn keeps_selected_merge_attributes_when_already_present() {
        let submissions = submission_entity();
        let submission = submission();
        let request = submissions
            .merge(&submission)
            .keep("id")
            .keep("createdAt")
            .request()
            .unwrap();
        let update = request.as_input().get_update_expression().as_deref();

        assert_eq!(
            update,
            Some(
                "SET #m0 = if_not_exists(#m0, :m0), #m1 = if_not_exists(#m1, :m1), #m2 = :m2, #m3 = :m3"
            )
        );
    }

    #[test]
    fn excludes_primary_key_attributes_from_merge_updates() {
        let submissions = submission_entity();
        let submission = submission();
        let request = submissions.merge(&submission).request().unwrap();
        let input = request.as_input();
        let key = input.get_key().as_ref().unwrap();
        let names = input.get_expression_attribute_names().as_ref().unwrap();

        assert_eq!(key["userId"].as_s().unwrap(), "user");
        assert_eq!(key["problemId"].as_s().unwrap(), "problem");
        assert!(!names
            .values()
            .any(|name| name == "userId" || name == "problemId"));
    }

    #[test]
    fn aliases_reserved_merge_attribute_names() {
        let submissions = submission_entity();
        let submission = submission();
        let request = submissions.merge(&submission).request().unwrap();
        let input = request.as_input();
        let update = input.get_update_expression().as_deref().unwrap();
        let names = input.get_expression_attribute_names().as_ref().unwrap();

        assert!(!update.contains("type"));
        assert_eq!(names["#m3"], "type");
    }

    #[tokio::test]
    async fn rejects_a_second_merge_condition_at_run_time() {
        let submissions = submission_entity();
        let submission = submission();
        let error = submissions
            .merge(&submission)
            .when(Expression::new("attribute_exists(userId)"))
            .when(Expression::new("attribute_exists(problemId)"))
            .run()
            .await
            .unwrap_err();

        assert!(error.to_string().contains("more than one condition"));
    }

    #[tokio::test]
    async fn rejects_merge_condition_placeholder_collisions_at_run_time() {
        let submissions = submission_entity();
        let submission = submission();
        let error = submissions
            .merge(&submission)
            .when(Expression::new("#m0 = :expected").name("#m0", "createdAt"))
            .run()
            .await
            .unwrap_err();

        assert!(error.to_string().contains("placeholder"));
    }

    #[test]
    fn rejects_unknown_merge_keep_attributes() {
        let submissions = submission_entity();
        let submission = submission();
        let error = submissions
            .merge(&submission)
            .keep("missing")
            .request()
            .err()
            .unwrap();

        assert!(error.to_string().contains("unknown merge keep attribute"));
    }

    #[test]
    fn rejects_merges_without_non_key_attributes() {
        let keys = key_only_entity();
        let key = KeyOnlyTable { id: "key".into() };
        let error = keys.merge(&key).request().err().unwrap();

        assert!(error.to_string().contains("at least one attribute"));
    }

    #[test]
    fn fetch_requests_all_new_attributes() {
        let submissions = submission_entity();
        let submission = submission();
        let request = submissions.merge(&submission).fetch_request().unwrap();
        let input = request.as_input();

        assert_eq!(
            input.get_return_values().as_ref(),
            Some(&ReturnValue::AllNew)
        );
    }

    #[test]
    fn update_fetch_requests_all_new_attributes() {
        let records = entity();
        let request = update(&records).fetch_request().unwrap();
        let input = request.as_input();

        assert_eq!(
            input.get_return_values().as_ref(),
            Some(&ReturnValue::AllNew)
        );
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

    fn submission_entity() -> Entity<SubmissionTable> {
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .build();
        let client = Client::from_conf(config);

        Entity::new(&client, "Submissions")
    }

    fn key_only_entity() -> Entity<KeyOnlyTable> {
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .build();
        let client = Client::from_conf(config);

        Entity::new(&client, "Keys")
    }

    fn submission() -> SubmissionTable {
        SubmissionTable {
            user_id: "user".into(),
            problem_id: "problem".into(),
            id: "submission".into(),
            created_at: 1,
            kind: "CHOICE".into(),
            score: 100,
        }
    }
}
