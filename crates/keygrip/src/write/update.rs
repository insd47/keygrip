use super::{applied, clause, invalid, Clause, Condition, Pending, Slot};
use crate::binding::Bindings;
use crate::expression::Expression;
use crate::key::document_key;
use crate::{item, Entity, Result, Schema};
use aws_sdk_dynamodb::operation::update_item::builders::UpdateItemFluentBuilder;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use std::collections::HashMap;
use std::future::{Future, IntoFuture};

/// An in-place update of the item at one key.
///
/// Built by [`Entity::update`] from an [`Expression`], or by
/// [`Entity::merge`] from a whole value. Awaiting it resolves to `true` when
/// the update applied and `false` when its [`Condition`] was rejected;
/// [`fetch`](Self::fetch) returns the stored item instead:
///
/// ```no_run
/// use keygrip::expression::Expression;
/// use keygrip::{Entity, Result, Schema};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// #[entity(pk(id))]
/// struct SubmissionTable {
///     id: String,
///     score: i64,
/// }
///
/// async fn improve(submissions: &Entity<SubmissionTable>, id: &str, score: i64) -> Result<bool> {
///     submissions
///         .update(id, Expression::new("SET score = :score").value(":score", &score))
///         .when(
///             Expression::new("attribute_not_exists(score) OR score < :floor")
///                 .value(":floor", &score),
///         )
///         .await
/// }
/// ```
///
/// An update expression and its condition must bind distinct placeholders.
#[must_use = "a write does nothing until it is awaited"]
pub struct Update<'a, E: Schema> {
    entity: &'a Entity<E>,
    key: HashMap<String, AttributeValue>,
    body: Body<'a, E>,
    condition: Slot,
    problem: Option<String>,
}

enum Body<'a, E> {
    Expression(Expression),
    Merge { value: &'a E, keep: Vec<String> },
}

pub(crate) struct Compiled {
    pub(crate) table: String,
    pub(crate) key: HashMap<String, AttributeValue>,
    pub(crate) update: String,
    pub(crate) clause: Clause,
}

impl<'a, E: Schema> Update<'a, E> {
    pub(crate) fn expression(
        entity: &'a Entity<E>,
        key: HashMap<String, AttributeValue>,
        expression: Expression,
    ) -> Self {
        Self::new(entity, key, Body::Expression(expression))
    }

    pub(crate) fn merge(entity: &'a Entity<E>, value: &'a E) -> Self {
        let key = document_key(E::parts(value.primary()));

        Self::new(
            entity,
            key,
            Body::Merge {
                value,
                keep: Vec::new(),
            },
        )
    }

    fn new(entity: &'a Entity<E>, key: HashMap<String, AttributeValue>, body: Body<'a, E>) -> Self {
        Self {
            entity,
            key,
            body,
            condition: Slot::default(),
            problem: None,
        }
    }

    /// Writes `attribute` only when the stored item does not already have it
    /// (`if_not_exists`).
    ///
    /// Applies to [`merge`](Entity::merge) updates only; on an expression
    /// update, or with an attribute the value does not serialize, it fails
    /// with [`Error::Invalid`](crate::Error::Invalid) when the write runs.
    pub fn keep(mut self, attribute: impl Into<String>) -> Self {
        match &mut self.body {
            Body::Merge { keep, .. } => keep.push(attribute.into()),
            Body::Expression(_) => {
                self.problem
                    .get_or_insert_with(|| "keep applies only to a merge".into());
            }
        }

        self
    }

    /// Attaches the condition that must hold for the update to apply.
    ///
    /// At most one condition may be attached; a second one fails with
    /// [`Error::Invalid`](crate::Error::Invalid) when the write runs.
    pub fn when(mut self, condition: impl Into<Condition>) -> Self {
        self.condition.attach(condition.into());
        self
    }

    /// Applies the update, resolving to `false` when its condition is
    /// rejected.
    ///
    /// Awaiting the `Update` directly does the same.
    pub fn run(self) -> impl Future<Output = Result<bool>> + Send + 'a {
        let request = self.request();

        async move { Ok(applied(request?.send().await)?.is_some()) }
    }

    /// Applies the update and returns the stored item (`ALL_NEW`), or `None`
    /// when its condition is rejected.
    pub fn fetch(self) -> impl Future<Output = Result<Option<E>>> + Send + 'a {
        let request = self.fetch_request();

        async move {
            match applied(request?.send().await)? {
                Some(output) => item::option(output.attributes),
                None => Ok(None),
            }
        }
    }

    pub(crate) fn compile(self) -> Result<Compiled> {
        if let Some(problem) = self.problem {
            return Err(invalid(problem));
        }

        let condition = self.condition.resolve::<E>()?;
        let update = match self.body {
            Body::Expression(expression) => expression.compile()?,
            Body::Merge { value, keep } => merge(value, &self.key, &keep)?,
        };
        let (update, clause) = clause(Some(update), condition)?;

        Ok(Compiled {
            table: self.entity.name().into(),
            key: self.key,
            update: update.unwrap_or_default(),
            clause,
        })
    }

    fn fetch_request(self) -> Result<UpdateItemFluentBuilder> {
        Ok(self.request()?.return_values(ReturnValue::AllNew))
    }

    fn request(self) -> Result<UpdateItemFluentBuilder> {
        let entity = self.entity;
        let Compiled {
            table,
            key,
            update,
            clause,
        } = self.compile()?;

        Ok(entity
            .client()
            .update_item()
            .table_name(table)
            .set_key(Some(key))
            .update_expression(update)
            .set_condition_expression(clause.condition)
            .set_expression_attribute_names(clause.names)
            .set_expression_attribute_values(clause.values))
    }
}

impl<'a, E: Schema> IntoFuture for Update<'a, E> {
    type Output = Result<bool>;
    type IntoFuture = Pending<'a, bool>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// Compiles a whole value into `SET` assignments for every serialized
/// non-key attribute, in attribute-name order.
fn merge<E: Schema>(
    value: &E,
    key: &HashMap<String, AttributeValue>,
    keep: &[String],
) -> Result<Bindings> {
    let mut document = item::to(value)?;

    for attribute in key.keys() {
        document.remove(attribute);
    }

    for attribute in keep {
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
        let assignment = if keep.contains(&attribute) {
            format!("{name} = if_not_exists({name}, {value_name})")
        } else {
            format!("{name} = {value_name}")
        };

        assignments.push(assignment);
        names.insert(name, attribute);
        values.insert(value_name, value);
    }

    Ok(Bindings {
        statement: format!("SET {}", assignments.join(", ")),
        names,
        values,
    })
}

#[cfg(test)]
mod tests {
    use super::Update;
    use crate::expression::Expression;
    use crate::{Condition, Entity, Error};
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
                    .value(":active", &true)
                    .value(":tags", &["tag"]),
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
                    .value(":next", "next"),
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
    fn serializes_put_values_with_composed_keys() {
        let records = entity();
        let record = RecordTable {
            scope: "contest".into(),
            owner: "user".into(),
            kind: "submission".into(),
            id: "one".into(),
            active: true,
        };
        let put = records
            .put(&record)
            .when(Condition::absent())
            .compile()
            .unwrap();
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

    #[test]
    fn resolves_existence_conditions_against_the_key_schema() {
        let submissions = submission_entity();
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

    #[test]
    fn rejects_keep_on_expression_updates() {
        let records = entity();
        let error = update(&records).keep("active").compile().err().unwrap();

        assert!(matches!(error, Error::Invalid(detail) if detail.contains("only to a merge")));
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
            Expression::new("SET active = :active").value(":active", &true),
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
