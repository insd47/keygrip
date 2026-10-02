mod common;

use common::{client, ok, replay};
use keygrip::{Entity, Error, Schema};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
#[entity(pk(id), index(name = "byName", pk(name)))]
struct UserTable {
    id: String,
    name: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
#[entity(pk(user_id), sk("run", problem_id, id))]
#[serde(rename_all = "camelCase")]
struct RunTable {
    user_id: String,
    problem_id: String,
    id: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
#[entity(pk(user_id), sk("gate"))]
#[serde(rename_all = "camelCase")]
struct GateTable {
    user_id: String,
    rev: i64,
}

#[tokio::test]
async fn find_reads_consistently_and_decodes_the_item() {
    let (client, captured) = client(ok(r#"{"Item":{"id":{"S":"one"},"name":{"S":"Ada"}}}"#));
    let users = Entity::<UserTable>::new(&client, "Users");

    assert_eq!(users.find("one").await.unwrap(), Some(user("one")));
    assert_eq!(
        captured.body(),
        json!({ "TableName": "Users", "Key": { "id": { "S": "one" } }, "ConsistentRead": true })
    );
}

#[tokio::test]
async fn find_returns_none_for_a_missing_item() {
    let (client, _) = client(ok("{}"));
    let users = Entity::<UserTable>::new(&client, "Users");

    assert_eq!(users.find("one").await.unwrap(), None);
}

#[tokio::test]
async fn get_fails_with_not_found_for_a_missing_item() {
    let (client, _) = client(ok("{}"));
    let users = Entity::<UserTable>::new(&client, "Users");

    assert!(matches!(users.get("one").await.unwrap_err(), Error::NotFound(_)));
}

#[tokio::test]
async fn items_that_do_not_fit_the_model_are_invalid() {
    let (client, _) = client(ok(r#"{"Item":{"id":{"S":"one"}}}"#));
    let users = Entity::<UserTable>::new(&client, "Users");

    assert!(matches!(users.find("one").await.unwrap_err(), Error::Invalid(_)));
}

#[tokio::test]
async fn queries_stay_inside_the_sort_key_space() {
    let items = r#"{"Items":[{"pk":{"S":"user"},"sk":{"S":"run#p1#a"},"userId":{"S":"user"},"problemId":{"S":"p1"},"id":{"S":"a"}}]}"#;
    let (client, captured) = client(ok(items));
    let runs = Entity::<RunTable>::new(&client, "Executions");
    let found = runs.query("user").prefix("p1#").all().await.unwrap();

    assert_eq!(
        found,
        [RunTable {
            user_id: "user".into(),
            problem_id: "p1".into(),
            id: "a".into(),
        }]
    );
    assert_eq!(
        captured.body(),
        json!({
            "TableName": "Executions",
            "KeyConditionExpression": "#partition = :partition AND begins_with(#sort, :sort)",
            "ExpressionAttributeNames": { "#partition": "pk", "#sort": "sk" },
            "ExpressionAttributeValues": { ":partition": { "S": "user" }, ":sort": { "S": "run#p1#" } },
            "ScanIndexForward": true,
            "ConsistentRead": false,
        })
    );
}

#[tokio::test]
async fn pages_clamp_the_limit_and_return_the_cursor() {
    let (client, captured) = client(ok(r#"{"Items":[],"LastEvaluatedKey":{"id":{"S":"one"}}}"#));
    let users = Entity::<UserTable>::new(&client, "Users");
    let page = users.query("one").page(None, u32::MAX).await.unwrap();

    assert_eq!(captured.body()["Limit"], i32::MAX);
    assert!(page.items.is_empty());
    assert_eq!(page.cursor.unwrap()["id"].as_s().unwrap(), "one");
}

#[tokio::test]
async fn consistent_index_queries_are_invalid_before_sending() {
    let (client, _) = client(ok("{}"));
    let users = Entity::<UserTable>::new(&client, "Users");
    let failure = users
        .query("Ada")
        .index(&UserTable::BY_NAME)
        .consistent()
        .all()
        .await
        .unwrap_err();

    assert!(matches!(failure, Error::Invalid(detail) if detail.contains("consistent")));
}

#[tokio::test]
async fn scans_filter_to_the_sort_key_space() {
    let items = r#"{"Items":[{"pk":{"S":"user"},"sk":{"S":"gate"},"userId":{"S":"user"},"rev":{"N":"2"}}]}"#;
    let (client, captured) = client(ok(items));
    let gates = Entity::<GateTable>::new(&client, "Executions");

    assert_eq!(
        gates.scan().await.unwrap(),
        [GateTable {
            user_id: "user".into(),
            rev: 2,
        }]
    );
    assert_eq!(
        captured.body(),
        json!({
            "TableName": "Executions",
            "FilterExpression": "#sort = :sort",
            "ExpressionAttributeNames": { "#sort": "sk" },
            "ExpressionAttributeValues": { ":sort": { "S": "gate" } },
        })
    );
}

#[tokio::test]
async fn batches_split_into_requests_of_100_keys() {
    let (client, replay) = replay(vec![
        ok(r#"{"Responses":{"Users":[{"id":{"S":"0"},"name":{"S":"Ada"}}]}}"#),
        ok(r#"{"Responses":{"Users":[{"id":{"S":"100"},"name":{"S":"Ada"}}]}}"#),
    ]);
    let users = Entity::<UserTable>::new(&client, "Users");
    let ids = (0..101).map(|id| id.to_string()).collect::<Vec<_>>();
    let found = users.batch(ids.iter().map(String::as_str)).await.unwrap();
    let sizes = replay
        .bodies()
        .iter()
        .map(|body| body["RequestItems"]["Users"]["Keys"].as_array().unwrap().len())
        .collect::<Vec<_>>();

    assert_eq!(sizes, [100, 1]);
    assert_eq!(found, [user("0"), user("100")]);
}

#[tokio::test]
async fn batches_with_unprocessed_keys_are_unavailable() {
    let unprocessed = json!({
        "Responses": { "Users": [] },
        "UnprocessedKeys": { "Users": { "Keys": [{ "id": { "S": "one" } }] } },
    });
    let (client, _) = client(ok(&unprocessed.to_string()));
    let users = Entity::<UserTable>::new(&client, "Users");

    assert!(matches!(users.batch(["one"]).await.unwrap_err(), Error::Unavailable(_)));
}

fn user(id: &str) -> UserTable {
    UserTable {
        id: id.into(),
        name: "Ada".into(),
    }
}
