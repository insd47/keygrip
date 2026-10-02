use super::{applied, clause, invalid, Clause, Condition, Pending, Slot};
use crate::binding::Bindings;
use crate::key::document_key;
use crate::transaction::Step;
use crate::{item, Entity, Expression, Result, Schema};
use aws_sdk_dynamodb::operation::update_item::builders::UpdateItemFluentBuilder;
use aws_sdk_dynamodb::types::{self, AttributeValue, ReturnValue, TransactWriteItem};
use std::collections::HashMap;
use std::future::{Future, IntoFuture};

/// An in-place update of the item at one key.
///
/// Built by [`Entity::update`] from an [`Expression`]. Awaiting it resolves to `true` when
/// the update applied and `false` when its [`Condition`] was rejected;
/// [`fetch`](Self::fetch) returns the stored item instead:
///
/// ```no_run
/// use keygrip::{Entity, Expression, Result, Schema};
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
/// An update expression and its condition may share a placeholder bound to
/// the same target; binding it to different targets fails with
/// [`Error::Invalid`](crate::Error::Invalid) when the write runs.
#[must_use = "a write does nothing until it is awaited"]
pub struct Update<'a, E: Schema> {
    entity: &'a Entity<E>,
    key: HashMap<String, AttributeValue>,
    body: Body<'a, E>,
    condition: Slot,
}

enum Body<'a, E> {
    Expression(Expression),
    Merge { value: &'a E, keep: Vec<String> },
}

struct Compiled {
    table: String,
    key: HashMap<String, AttributeValue>,
    update: String,
    clause: Clause,
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
        }
    }

    /// Adds `attribute` to a merge body's `if_not_exists` set; built only by
    /// [`Merge::keep`](crate::Merge::keep).
    pub(crate) fn keep(mut self, attribute: String) -> Self {
        if let Body::Merge { keep, .. } = &mut self.body {
            keep.push(attribute);
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

    fn compile(self) -> Result<Compiled> {
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
fn merge<E: Schema>(value: &E, key: &HashMap<String, AttributeValue>, keep: &[String]) -> Result<Bindings> {
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

impl<E: Schema> From<Update<'_, E>> for Step {
    fn from(write: Update<'_, E>) -> Self {
        let client = write.entity.client().clone();
        let item = write.compile().and_then(
            |Compiled {
                 table,
                 key,
                 update,
                 clause,
             }| {
                let update = types::Update::builder()
                    .table_name(table)
                    .set_key(Some(key))
                    .update_expression(update)
                    .set_condition_expression(clause.condition)
                    .set_expression_attribute_names(clause.names)
                    .set_expression_attribute_values(clause.values)
                    .build()
                    .map_err(|error| invalid(error.to_string()))?;

                Ok(TransactWriteItem::builder().update(update).build())
            },
        );

        Step { item, client }
    }
}
