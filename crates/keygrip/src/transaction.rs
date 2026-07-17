//! Ordered atomic writes across tables, for extension code implementing
//! multi-item invariants with `TransactWriteItems`.
//!
//! A [`Transaction`] is pure step data: it holds no client, and
//! [`run`](Transaction::run) receives one at execution time. It completes the
//! toolkit's write spectrum: [`occ`](crate::occ) retries a single-item
//! read-modify-write, [`write`](crate::write) performs one conditional write,
//! and a transaction commits writes to several items atomically.
//!
//! Each step's [`Expression`] bindings are isolated from every other step's,
//! so placeholders may be reused freely across steps; only an update sharing
//! a placeholder with its own condition is an error. Condition failures are
//! interpreted by [`label`](Transaction::label) rather than by position, so
//! inserting an optional step never shifts the meaning of a cancellation:
//!
//! ```no_run
//! use aws_sdk_dynamodb::Client;
//! use keygrip::expression::Expression;
//! use keygrip::transaction::{Transaction, TransactionError};
//! use keygrip::{Entity, Schema};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize, Schema)]
//! #[entity(pk(token_hash))]
//! #[serde(rename_all = "camelCase")]
//! struct SessionTable {
//!     token_hash: String,
//!     user_id: String,
//! }
//! # #[derive(Debug, Serialize, Deserialize, Schema)]
//! # #[entity(pk(id))]
//! # struct UserTable {
//! #     id: String,
//! # }
//!
//! async fn rotate(
//!     client: &Client,
//!     session: SessionTable,
//!     previous: &str,
//! ) -> Result<(), TransactionError> {
//!     let sessions = Entity::<SessionTable>::new(client, "Sessions");
//!     let users = Entity::<UserTable>::new(client, "Users");
//!
//!     let result = Transaction::new()
//!         .put(&sessions, &session)?
//!         .when(Expression::new("attribute_not_exists(tokenHash)"))
//!         .update(
//!             &users,
//!             session.user_id.as_str(),
//!             Expression::new("SET #session = :session")
//!                 .name("#session", "session")
//!                 .string(":session", &session.token_hash),
//!         )?
//!         .when(
//!             Expression::new("#session = :previous")
//!                 .name("#session", "session")
//!                 .string(":previous", previous),
//!         )
//!         .label("pointer")
//!         .delete(&sessions, previous)?
//!         .run(client)
//!         .await;
//!
//!     if let Err(error) = &result {
//!         if error.failed("pointer") {
//!             // the user's session pointer moved — another rotation won
//!         }
//!     }
//!
//!     result
//! }
//! ```

pub use crate::expression::Expression;

use crate::binding::{Bindings, BoundExpression};
use crate::key::document_key;
use crate::{item, Entity, Error, Schema};
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
use aws_sdk_dynamodb::types::{
    AttributeValue, CancellationReason, Delete, Put, TransactWriteItem, Update,
};
use aws_sdk_dynamodb::Client;
use std::collections::HashMap;

const CONDITION_FAILED: &str = "ConditionalCheckFailed";

/// Why a transaction failed to assemble or run.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct TransactionError(TransactionFailure);

impl TransactionError {
    /// Whether the step labeled `label` canceled the transaction with a
    /// rejected condition.
    ///
    /// Unlabeled steps cannot be interrogated, and anything other than a
    /// cancellation — assembly errors, transport failures — returns `false`.
    pub fn failed(&self, label: &str) -> bool {
        let TransactionFailure::Canceled { labels, reasons } = &self.0 else {
            return false;
        };
        let Some(index) = labels.get(label) else {
            return false;
        };

        reasons.get(*index).and_then(Option::as_deref) == Some(CONDITION_FAILED)
    }

    fn invalid(detail: impl Into<String>) -> Self {
        Self(TransactionFailure::Invalid(detail.into()))
    }

    fn database(error: Error) -> Self {
        Self(TransactionFailure::Database(error))
    }

    fn unavailable(error: impl std::fmt::Display) -> Self {
        Self(TransactionFailure::Unavailable(error.to_string()))
    }

    fn canceled(reasons: &[CancellationReason], labels: HashMap<&'static str, usize>) -> Self {
        let reasons = reasons
            .iter()
            .map(|reason| reason.code().map(str::to_string))
            .collect();

        Self(TransactionFailure::Canceled { labels, reasons })
    }
}

impl From<TransactionError> for Error {
    fn from(error: TransactionError) -> Self {
        match error.0 {
            TransactionFailure::Database(error) => error,
            failure => Self::Unavailable(failure.to_string()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum TransactionFailure {
    #[error("invalid transaction: {0}")]
    Invalid(String),
    #[error("transaction canceled: {reasons:?}")]
    Canceled {
        labels: HashMap<&'static str, usize>,
        reasons: Vec<Option<String>>,
    },
    #[error(transparent)]
    Database(Error),
    #[error("database unavailable: {0}")]
    Unavailable(String),
}

/// An order-preserving builder of DynamoDB transactional writes.
#[derive(Debug, Default)]
pub struct Transaction {
    steps: Vec<Step>,
    problem: Option<String>,
}

impl Transaction {
    /// Creates an empty transaction; no client is involved until
    /// [`run`](Self::run).
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a step that stores the whole `value` in `entity`'s table.
    pub fn put<E: Schema>(
        self,
        entity: &Entity<E>,
        value: &E,
    ) -> std::result::Result<Self, TransactionError> {
        let mut transaction = self.ready()?;
        let mut document = item::to(value).map_err(TransactionError::database)?;
        document.extend(document_key(E::parts(value.primary())));
        transaction.steps.push(Step::new(Action::Put {
            table: entity.name().into(),
            document,
        }));

        Ok(transaction)
    }

    /// Appends a step that applies `expression` to the item at `primary`.
    pub fn update<'a, E>(
        self,
        entity: &Entity<E>,
        primary: impl Into<E::Key<'a>>,
        expression: Expression,
    ) -> std::result::Result<Self, TransactionError>
    where
        E: Schema + 'a,
    {
        let mut transaction = self.ready()?;
        transaction.steps.push(Step::new(Action::Update {
            table: entity.name().into(),
            key: document_key(E::parts(primary)),
            expression,
        }));

        Ok(transaction)
    }

    /// Appends a step that removes the item at `primary`.
    pub fn delete<'a, E>(
        self,
        entity: &Entity<E>,
        primary: impl Into<E::Key<'a>>,
    ) -> std::result::Result<Self, TransactionError>
    where
        E: Schema + 'a,
    {
        let mut transaction = self.ready()?;
        transaction.steps.push(Step::new(Action::Delete {
            table: entity.name().into(),
            key: document_key(E::parts(primary)),
        }));

        Ok(transaction)
    }

    /// Attaches a condition to the last step.
    ///
    /// Each step carries at most one condition, and an update may not share a
    /// placeholder with its own condition. Assembly errors surface at the
    /// next fallible step or at [`run`](Self::run).
    pub fn when(mut self, condition: Expression) -> Self {
        if self.problem.is_some() {
            return self;
        }

        let problem = match self.steps.last_mut() {
            Some(step) => step.when(condition).err(),
            None => Some("a condition requires a preceding step".into()),
        };

        if let Some(problem) = problem {
            self.problem = Some(problem);
        }

        self
    }

    /// Names the last step so a cancellation can be interrogated with
    /// [`TransactionError::failed`].
    ///
    /// Labels must be unique within the transaction. Assembly errors surface
    /// at the next fallible step or at [`run`](Self::run).
    pub fn label(mut self, label: &'static str) -> Self {
        if self.problem.is_some() {
            return self;
        }
        if self.steps.iter().any(|step| step.label == Some(label)) {
            self.problem = Some(format!("duplicate transaction label: {label}"));

            return self;
        }
        let Some(step) = self.steps.last_mut() else {
            self.problem = Some("a label requires a preceding step".into());

            return self;
        };
        if step.label.is_some() {
            self.problem = Some("a transaction step cannot have more than one label".into());

            return self;
        }

        step.label = Some(label);
        self
    }

    /// Sends the assembled steps as one atomic `TransactWriteItems` request.
    pub async fn run(self, client: &Client) -> std::result::Result<(), TransactionError> {
        let request = self.request()?;
        let labels = request.labels;

        client
            .transact_write_items()
            .set_transact_items(Some(request.writes))
            .send()
            .await
            .map_err(|error| {
                let Some(TransactWriteItemsError::TransactionCanceledException(cancellation)) =
                    error.as_service_error()
                else {
                    return TransactionError::unavailable(error);
                };

                TransactionError::canceled(cancellation.cancellation_reasons(), labels)
            })?;

        Ok(())
    }

    fn ready(mut self) -> std::result::Result<Self, TransactionError> {
        match self.problem.take() {
            Some(problem) => Err(TransactionError::invalid(problem)),
            None => Ok(self),
        }
    }

    fn request(mut self) -> std::result::Result<Request, TransactionError> {
        if let Some(problem) = self.problem.take() {
            return Err(TransactionError::invalid(problem));
        }
        if self.steps.is_empty() {
            return Err(TransactionError::invalid(
                "a transaction requires at least one step",
            ));
        }

        let mut labels = HashMap::new();
        let mut writes = Vec::with_capacity(self.steps.len());

        for (index, step) in self.steps.into_iter().enumerate() {
            if let Some(label) = step.label {
                labels.insert(label, index);
            }

            writes.push(step.write()?);
        }

        Ok(Request { labels, writes })
    }
}

#[derive(Debug)]
struct Request {
    labels: HashMap<&'static str, usize>,
    writes: Vec<TransactWriteItem>,
}

#[derive(Debug)]
struct Step {
    action: Action,
    condition: Option<Expression>,
    label: Option<&'static str>,
}

impl Step {
    fn new(action: Action) -> Self {
        Self {
            action,
            condition: None,
            label: None,
        }
    }

    fn when(&mut self, condition: Expression) -> std::result::Result<(), String> {
        if self.condition.is_some() {
            return Err("a transaction step cannot have more than one condition".into());
        }
        if let Action::Update { expression, .. } = &self.action {
            if let Some(collision) = expression.bindings().collision(condition.bindings()) {
                return Err(collision);
            }
        }

        self.condition = Some(condition);
        Ok(())
    }

    fn write(self) -> std::result::Result<TransactWriteItem, TransactionError> {
        match self.action {
            Action::Put { table, document } => {
                let mut put = Put::builder().table_name(table).set_item(Some(document));

                if let Some(condition) = self.condition {
                    let Bindings {
                        statement,
                        names,
                        values,
                    } = condition.into_bindings();
                    put = put
                        .condition_expression(statement)
                        .set_expression_attribute_names(present(names))
                        .set_expression_attribute_values(present(values));
                }

                let put = put.build().map_err(TransactionError::unavailable)?;
                Ok(TransactWriteItem::builder().put(put).build())
            }
            Action::Update {
                table,
                key,
                expression,
            } => {
                let Bindings {
                    statement,
                    mut names,
                    mut values,
                } = expression.into_bindings();
                let mut update = Update::builder()
                    .table_name(table)
                    .set_key(Some(key))
                    .update_expression(statement);

                if let Some(condition) = self.condition {
                    let condition = condition.into_bindings();
                    names.extend(condition.names);
                    values.extend(condition.values);
                    update = update.condition_expression(condition.statement);
                }

                let update = update
                    .set_expression_attribute_names(present(names))
                    .set_expression_attribute_values(present(values))
                    .build()
                    .map_err(TransactionError::unavailable)?;

                Ok(TransactWriteItem::builder().update(update).build())
            }
            Action::Delete { table, key } => {
                let mut delete = Delete::builder().table_name(table).set_key(Some(key));

                if let Some(condition) = self.condition {
                    let Bindings {
                        statement,
                        names,
                        values,
                    } = condition.into_bindings();
                    delete = delete
                        .condition_expression(statement)
                        .set_expression_attribute_names(present(names))
                        .set_expression_attribute_values(present(values));
                }

                let delete = delete.build().map_err(TransactionError::unavailable)?;
                Ok(TransactWriteItem::builder().delete(delete).build())
            }
        }
    }
}

#[derive(Debug)]
enum Action {
    Put {
        table: String,
        document: HashMap<String, AttributeValue>,
    },
    Update {
        table: String,
        key: HashMap<String, AttributeValue>,
        expression: Expression,
    },
    Delete {
        table: String,
        key: HashMap<String, AttributeValue>,
    },
}

fn present<K, V>(values: HashMap<K, V>) -> Option<HashMap<K, V>> {
    (!values.is_empty()).then_some(values)
}

#[cfg(test)]
mod tests {
    use super::{Expression, Transaction, TransactionError};
    use crate::Entity;
    use aws_sdk_dynamodb::config::BehaviorVersion;
    use aws_sdk_dynamodb::types::CancellationReason;
    use aws_sdk_dynamodb::{Client, Config};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(id))]
    struct RecordTable {
        id: String,
        value: String,
    }

    #[test]
    fn preserves_step_order() {
        let records = entity("Records");
        let record = record("one");
        let request = Transaction::new()
            .put(&records, &record)
            .unwrap()
            .when(Expression::new("attribute_not_exists(id)"))
            .update(
                &records,
                "two",
                Expression::new("SET #value = :value")
                    .name("#value", "value")
                    .string(":value", "next"),
            )
            .unwrap()
            .delete(&records, "three")
            .unwrap()
            .request()
            .unwrap();

        assert!(request.writes[0].put().is_some());
        assert!(request.writes[1].update().is_some());
        assert!(request.writes[2].delete().is_some());
    }

    #[test]
    fn optional_rotate_step_does_not_shift_label_interpretation() {
        let short = rotate(false).request().unwrap();
        let long = rotate(true).request().unwrap();

        assert_eq!(short.writes.len(), 2);
        assert_eq!(long.writes.len(), 3);
        assert_eq!(short.labels.get("pointer"), Some(&1));
        assert_eq!(long.labels.get("pointer"), Some(&1));
    }

    #[test]
    fn rejects_update_condition_placeholder_collisions() {
        let records = entity("Records");
        let expressions = [
            (
                Expression::new("SET #value = :next").name("#value", "value"),
                Expression::new("#value = :previous").name("#value", "previous"),
            ),
            (
                Expression::new("SET value = :value").string(":value", "next"),
                Expression::new("value = :value").string(":value", "previous"),
            ),
        ];

        for (update, condition) in expressions {
            let error = Transaction::new()
                .update(&records, "one", update)
                .unwrap()
                .when(condition)
                .request()
                .unwrap_err();

            assert!(error.to_string().contains("placeholder"));
        }
    }

    #[test]
    fn rejects_duplicate_labels() {
        let records = entity("Records");
        let error = Transaction::new()
            .delete(&records, "one")
            .unwrap()
            .label("pointer")
            .delete(&records, "two")
            .unwrap()
            .label("pointer")
            .request()
            .unwrap_err();

        assert!(error.to_string().contains("duplicate transaction label"));
    }

    #[test]
    fn maps_cancellation_reasons_to_labels() {
        let reasons = [
            CancellationReason::builder().code("None").build(),
            CancellationReason::builder()
                .code("ConditionalCheckFailed")
                .build(),
            CancellationReason::builder()
                .code("TransactionConflict")
                .build(),
        ];
        let labels = HashMap::from([("session", 0), ("pointer", 1), ("previous", 2)]);
        let error = TransactionError::canceled(&reasons, labels);

        assert!(!error.failed("session"));
        assert!(error.failed("pointer"));
        assert!(!error.failed("previous"));
        assert!(!error.failed("unknown"));
    }

    fn rotate(previous: bool) -> Transaction {
        let sessions = entity("Sessions");
        let users = entity("Users");
        let session = record("session");
        let transaction = Transaction::new()
            .put(&sessions, &session)
            .unwrap()
            .when(Expression::new("attribute_not_exists(id)"))
            .update(
                &users,
                "user",
                Expression::new("SET #session = :session")
                    .name("#session", "session")
                    .string(":session", "session"),
            )
            .unwrap()
            .when(
                Expression::new("#pointer = :previous")
                    .name("#pointer", "session")
                    .string(":previous", "previous"),
            )
            .label("pointer");

        if previous {
            transaction.delete(&sessions, "previous").unwrap()
        } else {
            transaction
        }
    }

    fn entity(name: &str) -> Entity<RecordTable> {
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .build();
        let client = Client::from_conf(config);

        Entity::new(&client, name)
    }

    fn record(id: &str) -> RecordTable {
        RecordTable {
            id: id.into(),
            value: "value".into(),
        }
    }
}
