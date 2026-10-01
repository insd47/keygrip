//! Ordered atomic writes across tables with `TransactWriteItems`.

mod step;

pub use step::Step;

use crate::{request, Error, Result};
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
use aws_sdk_dynamodb::types::{CancellationReason, TransactWriteItem};
use aws_sdk_dynamodb::Client;
use std::collections::HashMap;
use std::future::{Future, IntoFuture};
use std::pin::Pin;

const CONDITION_FAILED: &str = "ConditionalCheckFailed";

/// An order-preserving set of writes committed atomically.
///
/// A transaction assembles the same [`Put`](crate::Put),
/// [`Update`](crate::Update), and [`Delete`](crate::Delete) values that run
/// on their own, conditions included. Awaiting it sends the steps with the
/// client of the first write's [`Entity`](crate::Entity), as single writes
/// do; [`run`](Self::run) takes the client explicitly instead. Condition
/// rejection is a value, as for single writes: the [`Outcome`] says whether
/// the transaction committed and which [`label`](Self::label)ed steps were
/// rejected, so inserting an optional step never shifts the meaning of a
/// cancellation:
///
/// ```no_run
/// use aws_sdk_dynamodb::Client;
/// use keygrip::{Condition, Entity, Expression, Result, Schema, Transaction};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// #[entity(pk(token_hash))]
/// #[serde(rename_all = "camelCase")]
/// struct SessionTable {
///     token_hash: String,
///     user_id: String,
/// }
/// # #[derive(Debug, Serialize, Deserialize, Schema)]
/// # #[entity(pk(id))]
/// # struct UserTable {
/// #     id: String,
/// # }
///
/// async fn rotate(client: &Client, session: &SessionTable, previous: &str) -> Result<bool> {
///     let sessions = Entity::<SessionTable>::new(client, "Sessions");
///     let users = Entity::<UserTable>::new(client, "Users");
///     let pointer = Expression::new("SET #session = :session")
///         .name("#session", "session")
///         .value(":session", &session.token_hash);
///
///     let outcome = Transaction::new()
///         .add(sessions.put(session).when(Condition::absent()))
///         .add(
///             users.update(session.user_id.as_str(), pointer).when(
///                 Expression::new("#pointer = :previous")
///                     .name("#pointer", "session")
///                     .value(":previous", previous),
///             ),
///         )
///         .label("pointer")
///         .add(sessions.delete(previous))
///         .await?;
///
///     if outcome.rejected("pointer") {
///         // the user's session pointer moved — another rotation won
///     }
///
///     Ok(outcome.committed())
/// }
/// ```
///
/// Each step's placeholders are isolated from every other step's, so they may
/// be reused freely across steps. DynamoDB commits a transaction within one
/// account and region, so every write should come from entities sharing a
/// client.
#[derive(Debug, Default)]
#[must_use = "a transaction does nothing until it is awaited"]
pub struct Transaction {
    client: Option<Client>,
    items: Vec<TransactWriteItem>,
    labels: HashMap<&'static str, usize>,
    problem: Option<Error>,
}

impl Transaction {
    /// Creates an empty transaction.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a write as the next step.
    ///
    /// A write that fails to compile fails the transaction at
    /// [`run`](Self::run).
    #[allow(clippy::should_implement_trait)] // a builder step, not arithmetic
    pub fn add(mut self, write: impl Into<Step>) -> Self {
        let Step { item, client } = write.into();
        self.client.get_or_insert(client);

        match item {
            Ok(item) => self.items.push(item),
            Err(error) => self.fail(error),
        }

        self
    }

    /// Names the last step so the [`Outcome`] can report its rejection.
    ///
    /// Labels must be unique, and each step takes at most one; violations
    /// fail with [`Error::Invalid`] at [`run`](Self::run).
    pub fn label(mut self, label: &'static str) -> Self {
        let Some(last) = self.items.len().checked_sub(1) else {
            self.fail(invalid("a label requires a preceding step"));

            return self;
        };

        if self.labels.contains_key(label) {
            self.fail(invalid(format!("duplicate transaction label {label}")));
        } else if self.labels.values().any(|&index| index == last) {
            self.fail(invalid("a transaction step cannot have more than one label"));
        } else {
            self.labels.insert(label, last);
        }

        self
    }

    /// Sends the steps as one atomic `TransactWriteItems` request through
    /// `client`.
    ///
    /// Awaiting the transaction does the same with the client of its first
    /// write. Resolves to an [`Outcome`] when DynamoDB either commits the steps or
    /// rejects at least one condition. Cancellations for any other reason —
    /// conflicting transactions, throttling — fail with
    /// [`Error::Unavailable`].
    pub async fn run(self, client: &Client) -> Result<Outcome> {
        if let Some(problem) = self.problem {
            return Err(problem);
        }

        if self.items.is_empty() {
            return Err(invalid("a transaction requires at least one step"));
        }

        let result = client
            .transact_write_items()
            .set_transact_items(Some(self.items))
            .send()
            .await;
        let Err(error) = result else {
            return Ok(Outcome { rejected: None });
        };

        if let Some(TransactWriteItemsError::TransactionCanceledException(cancellation)) = error.as_service_error() {
            if let Some(outcome) = Outcome::canceled(cancellation.cancellation_reasons(), &self.labels) {
                return Ok(outcome);
            }
        }

        Err(request::unavailable(error))
    }

    fn fail(&mut self, error: Error) {
        self.problem.get_or_insert(error);
    }
}

impl IntoFuture for Transaction {
    type Output = Result<Outcome>;
    type IntoFuture = Pin<Box<dyn Future<Output = Result<Outcome>> + Send>>;

    fn into_future(mut self) -> Self::IntoFuture {
        Box::pin(async move {
            match self.client.take() {
                Some(client) => self.run(&client).await,
                None => Err(invalid("a transaction requires at least one step")),
            }
        })
    }
}

/// How a [`Transaction`] resolved: committed, or canceled by rejected
/// conditions.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a rejected transaction wrote nothing"]
pub struct Outcome {
    rejected: Option<Vec<&'static str>>,
}

impl Outcome {
    /// Whether every step was written.
    pub fn committed(&self) -> bool {
        self.rejected.is_none()
    }

    /// Whether the step labeled `label` canceled the transaction with a
    /// rejected condition.
    ///
    /// Unlabeled steps cannot be interrogated; check
    /// [`committed`](Self::committed) instead.
    pub fn rejected(&self, label: &str) -> bool {
        self.rejected.as_ref().is_some_and(|labels| labels.contains(&label))
    }

    /// An outcome for a cancellation that rejected at least one condition.
    fn canceled(reasons: &[CancellationReason], labels: &HashMap<&'static str, usize>) -> Option<Self> {
        let rejected = reasons
            .iter()
            .map(|reason| reason.code() == Some(CONDITION_FAILED))
            .collect::<Vec<_>>();

        if !rejected.contains(&true) {
            return None;
        }

        let labels = labels
            .iter()
            .filter(|(_, &index)| rejected.get(index) == Some(&true))
            .map(|(&label, _)| label)
            .collect();

        Some(Self { rejected: Some(labels) })
    }
}

fn invalid(detail: impl Into<String>) -> Error {
    Error::Invalid(format!("transaction: {}", detail.into()))
}

#[cfg(test)]
mod tests {
    use super::{Outcome, Transaction};
    use crate::Expression;
    use crate::{Condition, Entity, Error};
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
    fn preserves_step_order_and_conditions() {
        let records = entity("Records");
        let record = record("one");
        let transaction = Transaction::new()
            .add(records.put(&record).when(Condition::absent()))
            .add(
                records.update(
                    "two",
                    Expression::new("SET #value = :value")
                        .name("#value", "value")
                        .value(":value", "next"),
                ),
            )
            .add(records.delete("three"));

        assert!(transaction.problem.is_none());
        assert_eq!(
            transaction.items[0].put().unwrap().condition_expression(),
            Some("attribute_not_exists(#keygripKey)")
        );
        assert!(transaction.items[1].update().is_some());
        assert!(transaction.items[2].delete().is_some());
    }

    #[test]
    fn optional_steps_do_not_shift_labels() {
        let short = rotate(false);
        let long = rotate(true);

        assert_eq!(short.items.len(), 2);
        assert_eq!(long.items.len(), 3);
        assert_eq!(short.labels.get("pointer"), Some(&1));
        assert_eq!(long.labels.get("pointer"), Some(&1));
    }

    #[tokio::test]
    async fn surfaces_write_problems_at_run() {
        let records = entity("Records");
        let error = Transaction::new()
            .add(
                records
                    .update("one", Expression::new("SET value = :value").value(":value", "next"))
                    .when(Expression::new("value = :value").value(":value", "previous")),
            )
            .run(&client())
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Invalid(detail) if detail.contains("placeholder")));
    }

    #[tokio::test]
    async fn rejects_duplicate_labels() {
        let records = entity("Records");
        let error = Transaction::new()
            .add(records.delete("one"))
            .label("pointer")
            .add(records.delete("two"))
            .label("pointer")
            .run(&client())
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Invalid(detail) if detail.contains("duplicate")));
    }

    #[tokio::test]
    async fn awaits_with_the_first_write_client() {
        let records = entity("Records");
        let transaction = Transaction::new().add(records.delete("one"));

        assert!(transaction.client.is_some());

        let error = Transaction::new()
            .add(records.delete("one"))
            .label("pointer")
            .label("again")
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Invalid(detail) if detail.contains("more than one label")));
    }

    #[tokio::test]
    async fn rejects_awaiting_an_empty_transaction() {
        let error = Transaction::new().await.unwrap_err();

        assert!(matches!(error, Error::Invalid(detail) if detail.contains("at least one step")));
    }

    #[test]
    fn maps_rejected_conditions_to_labels() {
        let reasons = [
            reason("None"),
            reason("ConditionalCheckFailed"),
            reason("ConditionalCheckFailed"),
            reason("TransactionConflict"),
        ];
        let labels = HashMap::from([("session", 0), ("pointer", 1), ("previous", 3)]);
        let outcome = Outcome::canceled(&reasons, &labels).unwrap();

        assert!(!outcome.committed());
        assert!(!outcome.rejected("session"));
        assert!(outcome.rejected("pointer"));
        assert!(!outcome.rejected("previous"));
        assert!(!outcome.rejected("unknown"));
    }

    #[test]
    fn treats_cancellations_without_rejected_conditions_as_failures() {
        let reasons = [reason("None"), reason("TransactionConflict")];

        assert!(Outcome::canceled(&reasons, &HashMap::new()).is_none());
        assert!(Outcome { rejected: None }.committed());
    }

    fn rotate(previous: bool) -> Transaction {
        let sessions = entity("Sessions");
        let users = entity("Users");
        let session = record("session");
        let transaction = Transaction::new()
            .add(sessions.put(&session).when(Condition::absent()))
            .add(
                users
                    .update(
                        "user",
                        Expression::new("SET #session = :session")
                            .name("#session", "session")
                            .value(":session", "session"),
                    )
                    .when(
                        Expression::new("#pointer = :previous")
                            .name("#pointer", "session")
                            .value(":previous", "previous"),
                    ),
            )
            .label("pointer");

        if previous {
            transaction.add(sessions.delete("previous"))
        } else {
            transaction
        }
    }

    fn reason(code: &str) -> CancellationReason {
        CancellationReason::builder().code(code).build()
    }

    fn client() -> Client {
        let config = Config::builder().behavior_version(BehaviorVersion::latest()).build();

        Client::from_conf(config)
    }

    fn entity(name: &str) -> Entity<RecordTable> {
        Entity::new(&client(), name)
    }

    fn record(id: &str) -> RecordTable {
        RecordTable {
            id: id.into(),
            value: "value".into(),
        }
    }
}
