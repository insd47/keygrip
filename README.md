# KeyGrip

Typed, key-centric DynamoDB access for Rust.

`keygrip` derives a table's key schema from its storage model and gives you a live typed entity for the operations
DynamoDB is actually good at: key-based CRUD, batch reads, scans, and partition queries. Everything else — filters,
joins, cross-entity composition — is deliberately out of scope: access patterns belong to your code, key layouts belong
to your models.

```rust
use aws_sdk_dynamodb::Client;
use keygrip::{Entity, Result, Schema};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Schema)]
#[entity(pk(contest_id), sk(id))]
#[serde(rename_all = "camelCase")]
struct UserTable {
    contest_id: String,
    id: String,
    name: String,
}

async fn users(client: Client) -> Result<Vec<UserTable>> {
    let users = Entity::<UserTable>::new(&client, "Users");

    users.query("contest").newest().all().await
}
```

## Declaring schemas

A table declaration is a serde struct plus one `#[entity(…)]` attribute:

```rust
#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(user_id), sk(problem_id, kind, id),
    index(name = "byId", pk(id), sk(user_id)))]
struct ExecutionTable {
    user_id: String,
    problem_id: String,
    kind: Kind,
    id: String,
    // …
}
```

- `pk(field, …)` *(required)* and `sk(field, …)` *(optional)* list the fields composing each key.
- **Attribute naming**: a single-field key uses the field's camelCase name (`user_id` → `userId`). Once a component is
  composite, the synthetic names
  `pk`/`sk` are used and members are joined with `#` — the example stores
  `sk = "{problem_id}#{kind}#{id}"`.
- **Encoded key parts**: non-`String` fields implement `KeyPart` to define how they appear inside keys. The encoding
  stays ordinary, greppable code:

  ```rust
  impl KeyPart for Kind {
      fn part(&self) -> String {
          match self { Self::Submit => "S", Self::Test => "T" }.into()
      }
  }
  ```

- **Indexes**: each `index(…)` clause emits a constant (`ExecutionTable::BY_ID`) to pass to `Query::index`.
- **Prefix builders**: a composite sort key gets a generated
  `ExecutionTable::prefix(problem_id, kind)` returning a `begins_with`-ready string (`"{problem_id}#{kind}#"`).
- `name = "…"` overrides the display name used in error messages (default:
  the struct name minus a trailing `Table`).

Manual `impl Schema` remains valid for tables that break these conventions.

## Working with an entity

`Entity<E>` pairs a client with a table name:

```rust
let executions = Entity::<ExecutionTable>::new(&client, "Executions");

executions.get(( & user, & problem, & kind, & id)).await?;   // NotFound if absent
executions.find(( & user, & problem, & kind, & id)).await?;  // Option<E>
executions.create( & execution).await?;                   // fails if the key exists
executions.put( & execution).await?;                      // unconditional write
executions.delete(( & user, & problem, & kind, & id)).await?;
executions.scan().await?;                               // whole table; small tables only
executions.batch(keys).await?;                          // ≤100-key chunks, consistent reads
```

Keys are passed as a value or a tuple of values matching the declared key fields — never as attribute-name strings.

Queries are a typed transliteration of the DynamoDB Query API — partition equality, at most one sort-key condition, a
direction, pagination — and nothing DynamoDB cannot do natively in a single call:

```rust
// all submit executions for a problem, newest first, one page
executions
.query( & user)
.prefix(ExecutionTable::prefix( & problem, & Kind::Submit))
.newest()
.page(cursor, 20)
.await?;            // -> Page { items, cursor }

// point lookup through a GSI
executions
.query( & id)
.index( & ExecutionTable::BY_ID)
.eq( & user)
.page(None, 1)
.await?;
```

## Atomic writes

`transaction` assembles ordered writes without owning a client. Conditions stay on their write step, and labels let
callers interpret cancellation by domain name instead of by a positional index:

```rust
use keygrip::transaction::{Expression, Transaction};

Transaction::new()
    .put(&sessions, &session)?
    .when(Expression::new("attribute_not_exists(tokenHash)"))
    .update(
        &users,
        &user.id,
        Expression::new("SET #session = :session")
            .name("#session", "session")
            .string(":session", &session.token_hash),
    )?
    .when(
        Expression::new("#pointer = :previous")
            .name("#pointer", "session")
            .string(":previous", previous),
    )
    .label("pointer")
    .run(&client)
    .await?;
```

Labels must be unique. An update and its condition must also use distinct placeholder names; placeholders may be
reused freely by different steps because their bindings are isolated.

## Extending with your own operations

Domain operations — conditional updates, optimistic locking, and the contents of transactions — are where your
invariants live, so keygrip does not try to generalize them. Instead it hands you the pieces:

- `entity.client()` and `entity.name()` for issuing SDK calls against the same table;
- `attr` (attribute-value constructors), `item` (serde ↔ item conversion),
  `request` (SDK error mapping), `occ` (optimistic retry loop), and `transaction`
  (ordered atomic-write assembly with labeled condition failures).

Since Rust does not allow inherent impls on foreign types, wrap the entity in a thin newtype in your crate and attach
domain methods there:

```rust
pub struct Users(keygrip::Entity<UserTable>);

impl std::ops::Deref for Users {
    type Target = keygrip::Entity<UserTable>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl Users {
    pub async fn exit(&self, id: &str, now: i64) -> keygrip::Result<UserTable> {
        let response = self
            .client()
            .update_item()
            .table_name(self.name())
            .key("id", keygrip::attr::s(id))
            .update_expression("SET exited = :true, updatedAt = :now")
            .expression_attribute_values(":true", keygrip::attr::boolean(true))
            .expression_attribute_values(":now", keygrip::attr::n(now))
            .condition_expression("attribute_exists(id)")
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
            .send()
            .await
            .map_err(keygrip::request::unavailable)?;

        keygrip::item::from(response.attributes.unwrap_or_default())
    }
}
```

Generic operations pass through `Deref`; your invariants stay yours.

## Feature flags

- `dynamodb` *(default)* — the AWS SDK-backed entity, query, and extension toolkit.
- With `default-features = false`, only the schema vocabulary and the derive macro remain. Use this from model-only
  crates (DTO layers, Lambdas that must not compile the AWS SDK) that still need to name your table types.

## Versioning

Before 1.0, keygrip tracks the latest `aws-sdk-dynamodb` 1.x releases; an SDK change that breaks keygrip's public API
results in a minor version bump.

## License

[MIT](LICENSE)
