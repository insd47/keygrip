//! An offline DynamoDB: each test client answers one request with a canned
//! response and hands the request back for inspection.

#![allow(dead_code)] // each test crate uses a different subset

use aws_sdk_dynamodb::config::retry::RetryConfig;
use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::{Client, Config};
use aws_smithy_http_client::test_util::{capture_request, CaptureRequestReceiver};
use serde_json::Value;

/// A client whose single request receives `response`.
pub fn client(response: http::Response<String>) -> (Client, Captured) {
    let (http, receiver) = capture_request(Some(response.map(Into::into)));
    let config = Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .retry_config(RetryConfig::disabled())
        .http_client(http)
        .build();

    (Client::from_conf(config), Captured(receiver))
}

/// A successful response with a JSON body.
pub fn ok(body: &str) -> http::Response<String> {
    http::Response::builder().status(200).body(body.into()).unwrap()
}

/// A DynamoDB error response of `kind`, such as `ConditionalCheckFailedException`.
pub fn error(kind: &str) -> http::Response<String> {
    let body = format!(r#"{{"__type":"com.amazonaws.dynamodb.v20120810#{kind}","message":"{kind}"}}"#);

    http::Response::builder().status(400).body(body).unwrap()
}

/// A canceled transaction with one cancellation reason code per step.
pub fn canceled(codes: &[&str]) -> http::Response<String> {
    let reasons = codes
        .iter()
        .map(|code| serde_json::json!({ "Code": code }))
        .collect::<Vec<_>>();
    let body = serde_json::json!({
        "__type": "com.amazonaws.dynamodb.v20120810#TransactionCanceledException",
        "Message": "Transaction cancelled",
        "CancellationReasons": reasons,
    });

    http::Response::builder().status(400).body(body.to_string()).unwrap()
}

pub struct Captured(CaptureRequestReceiver);

impl Captured {
    /// The JSON body of the request the client sent.
    pub fn body(self) -> Value {
        let request = self.0.expect_request();

        serde_json::from_slice(request.body().bytes().expect("in-memory body")).unwrap()
    }
}
