mod common;

use aws_sdk_dynamodb::Client;
use common::{canceled, client, ok};
use keygrip::{Condition, Entity, Error, Expression, Schema, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, Schema)]
#[entity(pk(id))]
struct RecordTable {
    id: String,
    value: String,
}

#[tokio::test]
async fn commits_steps_in_order_with_their_conditions() {
    let (client, captured) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let next = Expression::new("SET #value = :value")
        .name("#value", "value")
        .value(":value", "next");
    let outcome = Transaction::new()
        .add(records.put(&record("one")).when(Condition::absent()))
        .add(records.update("two", next))
        .add(records.delete("three"))
        .await
        .unwrap();

    assert!(outcome.committed());
    // The SDK adds its own idempotency token; the items are what keygrip builds.
    assert_eq!(
        captured.body()["TransactItems"],
        json!([
                { "Put": {
                    "TableName": "Records",
                    "Item": { "id": { "S": "one" }, "value": { "S": "value" } },
                    "ConditionExpression": "attribute_not_exists(#keygripKey)",
                    "ExpressionAttributeNames": { "#keygripKey": "id" },
                } },
                { "Update": {
                    "TableName": "Records",
                    "Key": { "id": { "S": "two" } },
                    "UpdateExpression": "SET #value = :value",
                    "ExpressionAttributeNames": { "#value": "value" },
                    "ExpressionAttributeValues": { ":value": { "S": "next" } },
                } },
            { "Delete": { "TableName": "Records", "Key": { "id": { "S": "three" } } } },
        ])
    );
}

#[tokio::test]
async fn reports_rejected_steps_by_label_whatever_comes_before_them() {
    for optional in [false, true] {
        // DynamoDB reports one cancellation reason per step, in order.
        let codes: &[&str] = if optional {
            &["None", "None", "ConditionalCheckFailed"]
        } else {
            &["None", "ConditionalCheckFailed"]
        };
        let (client, _) = client(canceled(codes));
        let outcome = rotate(&client, optional).await.unwrap();

        assert!(!outcome.committed());
        assert!(outcome.rejected("pointer"));
        assert!(!outcome.rejected("session"));
    }
}

#[tokio::test]
async fn cancellations_without_a_rejected_condition_are_unavailable() {
    let (client, _) = client(canceled(&["None", "TransactionConflict"]));
    let failure = rotate(&client, false).await.unwrap_err();

    assert!(matches!(failure, Error::Unavailable(_)));
}

#[tokio::test]
async fn write_problems_surface_when_the_transaction_runs() {
    let (client, _) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let update = Expression::new("SET value = :value").value(":value", "next");
    let condition = Expression::new("value = :value").value(":value", "previous");
    let failure = Transaction::new()
        .add(records.update("one", update).when(condition))
        .await
        .unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains(":value")));
}

#[tokio::test]
async fn labels_must_be_unique() {
    let (client, _) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let failure = Transaction::new()
        .add(records.delete("one"))
        .label("pointer")
        .add(records.delete("two"))
        .label("pointer")
        .await
        .unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("duplicate transaction label pointer")));
}

#[tokio::test]
async fn a_step_takes_one_label() {
    let (client, _) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let failure = Transaction::new()
        .add(records.delete("one"))
        .label("pointer")
        .label("again")
        .await
        .unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("more than one label")));
}

#[tokio::test]
async fn an_empty_transaction_is_invalid() {
    let failure = Transaction::new().await.unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("at least one step")));
}

/// A session rotation whose second step, labeled `pointer`, moves the user's
/// session pointer; `optional` inserts an unlabeled step in front of it.
async fn rotate(client: &Client, optional: bool) -> keygrip::Result<keygrip::Outcome> {
    let sessions = Entity::<RecordTable>::new(client, "Sessions");
    let users = Entity::<RecordTable>::new(client, "Users");
    let pointer = Expression::new("SET #value = :next")
        .name("#value", "value")
        .value(":next", "session");
    let unchanged = Expression::new("#value = :previous")
        .name("#value", "value")
        .value(":previous", "previous");

    let mut transaction = Transaction::new();

    if optional {
        transaction = transaction.add(sessions.delete("previous"));
    }

    transaction
        .add(sessions.put(&record("session")).when(Condition::absent()))
        .label("session")
        .add(users.update("user", pointer).when(unchanged))
        .label("pointer")
        .await
}

fn record(id: &str) -> RecordTable {
    RecordTable {
        id: id.into(),
        value: "value".into(),
    }
}
