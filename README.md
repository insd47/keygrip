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

`Entity<E>` pairs a client with a table name. Reads run immediately:

```rust
let executions = Entity::<ExecutionTable>::new(&client, "Executions");

executions.get((&user, &problem, &kind, &id)).await?;   // NotFound if absent
executions.find((&user, &problem, &kind, &id)).await?;  // Option<E>
executions.scan().await?;                               // whole table; small tables only
executions.batch(keys).await?;                          // ≤100-key chunks, consistent reads
```

Keys are passed as a value or a tuple of values matching the declared key fields — never as attribute-name strings.

Queries are a typed transliteration of the DynamoDB Query API — partition equality, at most one sort-key condition, a
direction, pagination, consistency — and nothing DynamoDB cannot do natively in a single call:

```rust
// all submit executions for a problem, newest first, one page
executions
    .query(&user)
    .prefix(ExecutionTable::prefix(&problem, &Kind::Submit))
    .newest()
    .page(cursor, 20)
    .await?;            // -> Page { items, cursor }

// every execution, including one written a moment ago
executions.query(&user).consistent().all().await?;

// point lookup through a GSI (GSIs cannot be read consistently)
executions
    .query(&id)
    .index(&ExecutionTable::BY_ID)
    .eq(&user)
    .page(None, 1)
    .await?;
```

## Writes

Writes are values. `put`, `update`, `merge`, and `delete` return a `Put`, `Update`, or `Delete` that runs when
awaited:

```rust
use keygrip::{Condition, Expression};

executions.put(&execution).await?;                          // replaces any item at the key
executions.put(&execution).when(Condition::absent()).await?; // only if the key is free
executions.delete((&user, &problem, &kind, &id)).await?;

let applied = users
    .update(&user.id, Expression::new("SET exited = :exited").value(":exited", &true))
    .when(Condition::exists())
    .await?;
```

Each write takes at most one `Condition`: `Condition::absent()` and `Condition::exists()` check the entity's own key,
and any `Expression` converts into one. **A rejected condition is a value, not an error**: the write resolves to
`false`, so domain code can retry, fall back to a read, or report its own conflict. An unconditional write always
resolves to `true`.

`Expression` binds values through serde, so anything a model stores binds directly — strings, numbers, enums, lists,
nested structs. An update and its condition must use distinct placeholders.

`Update::fetch` applies the update and returns the stored item (`ALL_NEW`), or `None` when the condition is rejected.

`merge` builds an `Update` that sets every serialized non-key field of a value, and `keep` preserves selected fields
already stored (`if_not_exists`). Unlike `put`, it does not remove attributes absent from the new value; use it only
when every write preserves the same field set.

## Atomic writes

`Transaction` assembles the same write values, conditions included, into one `TransactWriteItems` request. Labels let
callers interpret a cancellation by domain name instead of by position:

```rust
use keygrip::{Condition, Expression, Transaction};

let outcome = Transaction::new()
    .add(sessions.put(&session).when(Condition::absent()))
    .add(
        users
            .update(&user.id, Expression::new("SET #session = :session")
                .name("#session", "session")
                .value(":session", &session.token_hash))
            .when(Expression::new("#pointer = :previous")
                .name("#pointer", "session")
                .value(":previous", previous)),
    )
    .label("pointer")
    .run(&client)
    .await?;

if outcome.rejected("pointer") {
    // another rotation won
}
```

Like single writes, rejected conditions resolve to an `Outcome` (`committed()`, `rejected(label)`) rather than an
error; cancellations for any other reason fail with `Error::Unavailable`. Labels must be unique; placeholders may be
reused freely by different steps.

## Errors

`Error` has three variants:

- `NotFound` — `Entity::get` found no item.
- `Invalid` — the operation cannot succeed as written: conflicting placeholders, a value or stored item that does not
  fit its serde model, or an unsupported combination such as a consistent GSI query. Retrying does not help.
- `Unavailable` — DynamoDB could not be reached or rejected the request transiently; it may succeed on retry.

Rejected conditions are never errors. Wrap `Error` in your own error type via `From` and add domain variants there.

## Extending with your own operations

Domain operations — conditional updates, optimistic locking, and the contents of transactions — are where your
invariants live, so keygrip does not try to generalize them. Retry policy belongs there too: loop on a write that
resolved to `false`.

Since Rust does not allow inherent impls on foreign types, wrap the entity in a thin newtype in your crate and attach
domain methods there:

```rust
pub struct Users(keygrip::Entity<UserTable>);

impl std::ops::Deref for Users {
    type Target = keygrip::Entity<UserTable>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl Users {
    pub async fn exit(&self, id: &str, now: i64) -> keygrip::Result<Option<UserTable>> {
        let exit = Expression::new("SET exited = :exited, updatedAt = :now")
            .value(":exited", &true)
            .value(":now", &now);

        self.update(id, exit).when(Condition::exists()).fetch().await
    }
}
```

Generic operations pass through `Deref`; your invariants stay yours. For item shapes the derive cannot express — say,
a per-partition marker item at a fixed sort key — implement `Schema` by hand and give that shape its own `Entity` on
the same table. `Entity::client()` and `Entity::name()` remain available for SDK operations outside the typed surface.

## Feature flags

- `dynamodb` *(default)* — the AWS SDK-backed entity, query, writes, and transactions.
- With `default-features = false`, only the schema vocabulary and the derive macro remain. Use this from model-only
  crates (DTO layers, Lambdas that must not compile the AWS SDK) that still need to name your table types.

## Versioning

Before 1.0, keygrip tracks the latest `aws-sdk-dynamodb` 1.x releases; an SDK change that breaks keygrip's public API
results in a minor version bump.

## License

[MIT](LICENSE)
