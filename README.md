# keygrip

`keygrip` is a typed, key-centric DynamoDB access layer for Rust. It derives key layouts from storage models and provides a small `Handle<E>` API for CRUD, batch reads, scans, and partition queries.

```rust
use aws_sdk_dynamodb::Client;
use keygrip::{Entity, Handle, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Entity)]
#[entity(pk(contest_id), sk(id))]
#[serde(rename_all = "camelCase")]
struct UserTable {
    contest_id: String,
    id: String,
    name: String,
}

async fn users(client: Client) -> Result<Vec<UserTable>> {
    let users = Handle::<UserTable>::new(client, "Users");

    users.query("contest").newest().all().await
}
```

The default `dynamodb` feature enables the AWS SDK-backed handle and extension toolkit. Without default features, the entity vocabulary and derive macro remain available for model-only crates.

Before `1.0`, `keygrip` tracks the latest `aws-sdk-dynamodb` 1.x releases. An SDK change that breaks the public API will result in a minor version bump.
