#![cfg(feature = "dynamodb")]

mod common;

use common::{client, error, ok};
use keygrip::{Condition, Entity, Error, Expression, Schema};
use serde::ser::Error as _;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
#[entity(pk(scope, owner), sk(kind, id))]
struct RecordTable {
    scope: String,
    owner: String,
    kind: String,
    id: String,
    active: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
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

#[derive(Debug, Serialize, Deserialize, Schema)]
#[entity(pk(id))]
struct KeyOnlyTable {
    id: String,
}

struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(S::Error::custom("refused"))
    }
}

#[tokio::test]
async fn put_writes_the_whole_item_with_its_encoded_key() {
    let (client, captured) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");

    assert!(records.put(&record()).when(Condition::absent()).await.unwrap());
    assert_eq!(
        captured.body(),
        json!({
            "TableName": "Records",
            "Item": {
                "pk": { "S": "contest#user" },
                "sk": { "S": "submission#one" },
                "scope": { "S": "contest" },
                "owner": { "S": "user" },
                "kind": { "S": "submission" },
                "id": { "S": "one" },
                "active": { "BOOL": true },
            },
            "ConditionExpression": "attribute_not_exists(#keygripKey)",
            "ExpressionAttributeNames": { "#keygripKey": "pk" },
        })
    );
}

#[tokio::test]
async fn delete_checks_existence_against_the_partition_attribute() {
    let (client, captured) = client(ok("{}"));
    let submissions = Entity::<SubmissionTable>::new(&client, "Submissions");

    assert!(submissions
        .delete(("user", "problem"))
        .when(Condition::exists())
        .await
        .unwrap());
    assert_eq!(
        captured.body(),
        json!({
            "TableName": "Submissions",
            "Key": { "userId": { "S": "user" }, "problemId": { "S": "problem" } },
            "ConditionExpression": "attribute_exists(#keygripKey)",
            "ExpressionAttributeNames": { "#keygripKey": "userId" },
        })
    );
}

#[tokio::test]
async fn rejected_conditions_resolve_to_false() {
    let (client, _) = client(error("ConditionalCheckFailedException"));
    let records = Entity::<RecordTable>::new(&client, "Records");

    assert!(!records.put(&record()).when(Condition::absent()).await.unwrap());
}

#[tokio::test]
async fn rejected_conditions_fetch_nothing() {
    let (client, _) = client(error("ConditionalCheckFailedException"));
    let submissions = Entity::<SubmissionTable>::new(&client, "Submissions");
    let stored = submissions
        .update(
            ("user", "problem"),
            Expression::new("SET score = :score").value(":score", &1),
        )
        .when(Condition::exists())
        .fetch()
        .await
        .unwrap();

    assert_eq!(stored, None);
}

#[tokio::test]
async fn fetch_returns_the_stored_item() {
    let stored = json!({
        "Attributes": {
            "userId": { "S": "user" },
            "problemId": { "S": "problem" },
            "id": { "S": "submission" },
            "createdAt": { "N": "1" },
            "type": { "S": "CHOICE" },
            "score": { "N": "100" },
        }
    });
    let (client, captured) = client(ok(&stored.to_string()));
    let submissions = Entity::<SubmissionTable>::new(&client, "Submissions");
    let fetched = submissions
        .update(
            ("user", "problem"),
            Expression::new("SET score = :score").value(":score", &100),
        )
        .fetch()
        .await
        .unwrap();

    assert_eq!(fetched, Some(submission()));
    assert_eq!(captured.body()["ReturnValues"], "ALL_NEW");
}

#[tokio::test]
async fn other_service_errors_are_unavailable() {
    let (client, _) = client(error("ResourceNotFoundException"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let failure = records.put(&record()).await.unwrap_err();

    assert!(matches!(failure, Error::Unavailable(_)));
}

#[tokio::test]
async fn update_and_condition_share_placeholders_bound_to_the_same_target() {
    let (client, captured) = client(ok("{}"));
    let submissions = Entity::<SubmissionTable>::new(&client, "Submissions");
    let update = Expression::new("SET score = :score, #best = :best")
        .name("#best", "best")
        .value(":score", &10)
        .value(":best", "submission");
    let condition = Expression::new("attribute_not_exists(#best) OR score < :score")
        .name("#best", "best")
        .value(":score", &10);

    assert!(submissions
        .update(("user", "problem"), update)
        .when(condition)
        .await
        .unwrap());
    assert_eq!(
        captured.body(),
        json!({
            "TableName": "Submissions",
            "Key": { "userId": { "S": "user" }, "problemId": { "S": "problem" } },
            "UpdateExpression": "SET score = :score, #best = :best",
            "ConditionExpression": "attribute_not_exists(#best) OR score < :score",
            "ExpressionAttributeNames": { "#best": "best" },
            "ExpressionAttributeValues": { ":score": { "N": "10" }, ":best": { "S": "submission" } },
        })
    );
}

#[tokio::test]
async fn placeholders_bound_to_different_targets_are_invalid() {
    let (client, _) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let update = Expression::new("SET #state = :next")
        .name("#state", "state")
        .value(":next", "next");
    let condition = Expression::new("#state = :next").name("#state", "previous");
    let failure = records
        .update(("contest", "user", "submission", "one"), update)
        .when(condition)
        .await
        .unwrap_err();

    assert!(matches!(
        failure,
        Error::Invalid(detail) if detail.contains(r#"#state is bound to "state" by the update and to "previous" by the condition"#)
    ));
}

#[tokio::test]
async fn a_second_condition_is_invalid() {
    let (client, _) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let failure = records
        .put(&record())
        .when(Condition::absent())
        .when(Condition::exists())
        .await
        .unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("more than one condition")));
}

#[tokio::test]
async fn unserializable_values_are_invalid() {
    let (client, _) = client(ok("{}"));
    let records = Entity::<RecordTable>::new(&client, "Records");
    let update = Expression::new("SET active = :active").value(":active", &Unserializable);
    let failure = records
        .update(("contest", "user", "submission", "one"), update)
        .await
        .unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains(":active")));
}

#[tokio::test]
async fn merge_sets_every_non_key_attribute_and_keeps_selected_ones() {
    let (client, captured) = client(ok("{}"));
    let submissions = Entity::<SubmissionTable>::new(&client, "Submissions");

    assert!(submissions
        .merge(&submission())
        .keep("id")
        .keep("createdAt")
        .await
        .unwrap());

    let body = captured.body();

    assert_eq!(
        body["Key"],
        json!({ "userId": { "S": "user" }, "problemId": { "S": "problem" } })
    );
    assert_eq!(
        assignments(&body),
        HashMap::from([
            ("createdAt".to_string(), (true, json!({ "N": "1" }))),
            ("id".to_string(), (true, json!({ "S": "submission" }))),
            ("score".to_string(), (false, json!({ "N": "100" }))),
            ("type".to_string(), (false, json!({ "S": "CHOICE" }))),
        ])
    );
}

#[tokio::test]
async fn merge_rejects_keeping_an_attribute_the_value_lacks() {
    let (client, _) = client(ok("{}"));
    let submissions = Entity::<SubmissionTable>::new(&client, "Submissions");
    let failure = submissions.merge(&submission()).keep("missing").await.unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("unknown merge keep attribute missing")));
}

#[tokio::test]
async fn merge_rejects_values_without_non_key_attributes() {
    let (client, _) = client(ok("{}"));
    let keys = Entity::<KeyOnlyTable>::new(&client, "Keys");
    let failure = keys.merge(&KeyOnlyTable { id: "key".into() }).await.unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("at least one attribute")));
}

#[tokio::test]
async fn merge_rejects_conditions_rebinding_its_placeholders() {
    let (client, _) = client(ok("{}"));
    let submissions = Entity::<SubmissionTable>::new(&client, "Submissions");
    let failure = submissions
        .merge(&submission())
        .when(Expression::new("#m0 = :expected").name("#m0", "id"))
        .await
        .unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("#m0")));
}

/// Resolves a merge's `SET` assignments to attribute name → (kept with
/// `if_not_exists`, value), independent of placeholder numbering.
fn assignments(body: &Value) -> HashMap<String, (bool, Value)> {
    let names = &body["ExpressionAttributeNames"];
    let values = &body["ExpressionAttributeValues"];
    let expression = body["UpdateExpression"].as_str().unwrap();

    // Every assignment starts with a name placeholder; `if_not_exists(#m0, :m0)` holds a ", " of its own.
    expression
        .strip_prefix("SET #")
        .unwrap()
        .split(", #")
        .map(|assignment| {
            let (name, value) = assignment.split_once(" = ").unwrap();
            let name = format!("#{name}");
            let kept = value.starts_with("if_not_exists(");
            let placeholder = value.trim_end_matches(')').rsplit(' ').next().unwrap();

            (
                names[&name].as_str().unwrap().to_string(),
                (kept, values[placeholder].clone()),
            )
        })
        .collect()
}

fn record() -> RecordTable {
    RecordTable {
        scope: "contest".into(),
        owner: "user".into(),
        kind: "submission".into(),
        id: "one".into(),
        active: true,
    }
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
