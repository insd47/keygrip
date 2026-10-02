# KeyGrip

Typed, key-centric DynamoDB access for Rust.

`keygrip` derives a table's key schema from its storage model and gives you a live typed entity for the operations
DynamoDB is actually good at: key-based CRUD, batch reads, scans, and partition queries. Everything else (filters,
joins, cross-entity composition) is deliberately out of scope: access patterns belong to your code, key layouts belong
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

## Installation

```sh
cargo add keygrip aws-sdk-dynamodb
cargo add serde --features derive
```

`keygrip` re-exports the `Schema` derive, so `keygrip-derive` is never a direct dependency. You build the
`aws_sdk_dynamodb::Client` yourself (usually through `aws-config`) and hand it to each entity.

## Declaring schemas

A table declaration is a serde struct plus one `#[entity(…)]` attribute:

```rust
#[derive(Serialize, Deserialize, Schema)]
#[entity(
    pk(user_id),
    sk(problem_id, kind, id),
    index(name = "byId", pk(id), sk(user_id)),
)]
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
  composite, the synthetic names `pk`/`sk` are used and members are joined with `#`. The example stores
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

- **Sort key spaces**: leading string literals in `sk(…)` are fixed parts, so several item shapes can share one table
  without seeing each other:

  ```rust
  #[entity(pk(user_id), sk("run", problem_id, id))]  // sk = "run#{problem_id}#{id}"
  struct RunTable { /* … */ }

  #[entity(pk(user_id), sk("gate"))]                 // sk = "gate", one item per partition
  struct GateTable { /* … */ }
  ```

  Literals are left out of the key arguments (`gates.find(&user)`), force the `pk`/`sk` attribute names, and confine
  queries and scans: `runs.query(&user)` only returns `run#…` items, and `prefix`, `eq`, and `after` apply inside that
  space. Queries through an index are not confined, since index sort keys take no literals.

  > Adding a literal to an existing schema changes its stored keys, so items written before can no longer be read.
- **Indexes**: each `index(…)` clause emits a constant (`ExecutionTable::BY_ID`) to pass to `Query::index`.
- **Prefix builders**: a composite sort key gets a generated `ExecutionTable::prefix(problem_id, kind)` returning a
  `begins_with`-ready string (`"{problem_id}#{kind}#"`).
- `name = "…"` overrides the display name used in error messages (default: the struct name minus a trailing `Table`).

Manual `impl Schema` remains valid for tables that break these conventions.

## Reading

`Entity<E>` pairs a client with a table name. Keys are passed as a value or a tuple of values matching the declared
key fields, never as attribute-name strings. Reads run immediately:

```rust
let executions = Entity::<ExecutionTable>::new(&client, "Executions");

executions.get((&user, &problem, &kind, &id)).await?;   // NotFound if absent
executions.find((&user, &problem, &kind, &id)).await?;  // Option<E>
executions.scan().await?;                               // whole table; small tables only
executions.batch(keys).await?;                          // ≤100-key chunks, consistent reads
```

Queries are a typed transliteration of the DynamoDB Query API (partition equality, at most one sort-key condition, a
direction, pagination, consistency) and nothing DynamoDB cannot do natively in a single call:

```rust
// All submit executions for a problem, newest first, one page at a time.
let page = executions
    .query(&user)
    .prefix(ExecutionTable::prefix(&problem, &Kind::Submit))
    .newest()
    .page(cursor, 20)
    .await?;            // Page { items, cursor }

// Everything after a known sort key, including items written a moment ago.
executions.query(&user).after(&last).consistent().all().await?;

// A point lookup through a GSI. GSIs cannot be read consistently.
executions
    .query(&id)
    .index(&ExecutionTable::BY_ID)
    .eq(&user)
    .page(None, 1)
    .await?;
```

## Writing

Writes are values. `put`, `update`, `merge`, and `delete` return a `Put`, `Update`, `Merge`, or `Delete` that runs when
awaited:

```rust
use keygrip::{Condition, Expression};

executions.put(&execution).await?;                           // replaces any item at the key
executions.put(&execution).when(Condition::absent()).await?; // only if the key is free
executions.delete((&user, &problem, &kind, &id)).await?;

let applied = users
    .update(&user.id, Expression::new("SET exited = :exited").value(":exited", &true))
    .when(Condition::exists())
    .await?;
```

Each write takes at most one `Condition`: `Condition::absent()` and `Condition::exists()` check the entity's own key,
and any `Expression` converts into one.

> A rejected condition is a value, not an error: the write resolves to `false`. Check it; `.await?;` alone silently
> ignores the rejection. An unconditional write always resolves to `true`.

- `Expression` binds values through serde, so anything a model stores binds directly: strings, numbers, enums, lists,
  nested structs. An update and its condition may share a placeholder bound to the same target.
- `Update::fetch` and `Merge::fetch` apply the write and return the stored item (`ALL_NEW`), or `None` when the condition
  is rejected.
- `merge` builds a `Merge` that sets every serialized non-key field of a value, and `keep` preserves selected fields
  already stored (`if_not_exists`). Unlike `put`, it does not remove attributes absent from the new value; use it only
  when every write preserves the same field set.

## Transactions

`Transaction` assembles the same write values, conditions included, into one `TransactWriteItems` request. Labels let
callers interpret a cancellation by domain name instead of by position:

```rust
use keygrip::{Condition, Expression, Transaction};

let pointer = Expression::new("SET #session = :session")
    .name("#session", "session")
    .value(":session", &session.token_hash);
let unchanged = Expression::new("#pointer = :previous")
    .name("#pointer", "session")
    .value(":previous", previous);

let outcome = Transaction::new()
    .add(sessions.put(&session).when(Condition::absent()))
    .add(users.update(&user.id, pointer).when(unchanged))
    .label("pointer")
    .await?;

if outcome.rejected("pointer") {
    // Another rotation won.
}
```

Awaiting a transaction (or calling `run()`) sends it with the client of its first write's entity. Like single
writes, rejected conditions resolve to an `Outcome` (`committed()`, `rejected(label)`) rather than an error;
cancellations for any other reason fail with `Error::Unavailable`. Labels must be unique; placeholders may be reused
freely by different steps.

## Errors

`Error` has three variants:

- `NotFound`: `Entity::get` found no item.
- `Invalid`: the operation cannot succeed as written, such as conflicting placeholders, a value or stored item that
  does not fit its serde model, or a consistent GSI query. Retrying does not help.
- `Unavailable`: DynamoDB could not be reached or rejected the request transiently. It may succeed on retry.

Rejected conditions are never errors. Wrap `Error` in your own error type via `From` and add domain variants there.
`Error` is `#[non_exhaustive]`, so a `match` on it needs a wildcard arm.

## Domain operations

Domain operations (conditional updates, optimistic locking, the contents of transactions) are where your invariants
live, so keygrip does not try to generalize them. Retry policy belongs there too: loop on a write that resolved to
`false`.

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

Generic operations pass through `Deref`; your invariants stay yours. For item shapes the derive cannot express,
implement `Schema` by hand and give that shape its own `Entity` on the same table. `Entity::client()` and `Entity::name()` remain available for SDK operations outside the typed surface.

## Versioning

keygrip follows semantic versioning; before 1.0, breaking changes bump the minor version. It tracks the latest
`aws-sdk-dynamodb` 1.x releases, and an SDK change that breaks keygrip's public API is a breaking change too. See the
[changelog](CHANGELOG.md) for every release and its migration notes.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)
