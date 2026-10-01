# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0](https://github.com/insd47/keygrip/releases/tag/v0.4.0) - 2026-09-26

0.4 fills the gaps in the typed API so the extension toolkit can leave the public surface. Writes are values with one
condition rule, and transactions assemble the same values.

### Added

- `Query::consistent` for strongly consistent queries (rejected with `Error::Invalid` on a GSI).

### Changed

- `Entity::put`, `update`, `merge`, and `delete` return `Put`, `Update`, or `Delete`, which run when awaited (or through
  `run`). A rejected condition resolves to `false` everywhere; `Update::fetch` resolves to `None`.
- `Condition` guards a write: `Condition::absent()`, `Condition::exists()`, or any `Expression`.
- `Expression::value` binds any `impl Serialize`, replacing `string`, `number`, `boolean`, and raw `AttributeValue`s.
- `Transaction::add` takes write values and `run` resolves to an `Outcome` (`committed()`, `rejected(label)`) instead of
  failing with `TransactionError`. Cancellations other than rejected conditions fail with `Error::Unavailable`.
- `Error::Invalid` holds failures that retrying cannot fix: conflicting placeholders, (de)serialization, unsupported
  combinations. `Error::Unavailable` is now transient failures only.
- `Expression`, `Transaction`, and `Outcome` are exported from the crate root.

### Removed

- `Entity::create`, `Entity::store`, and the immediate `put`/`delete`; `Error::Conflict`; `TransactionError`.
- The extension toolkit: `attr`, `item`, `request`, and `occ`.

### Migration

| 0.3                                                           | 0.4                                                                                                                                            |
|---------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------|
| `table.create(&x).await?` erroring with `Error::Conflict`     | `if !table.put(&x).when(Condition::absent()).await? { /* conflict */ }` — **check the `bool`; `.await?;` alone silently ignores the conflict** |
| `table.put(&x).await?`                                        | `table.put(&x).await?;` (always `true`)                                                                                                        |
| `table.delete(key).await?`                                    | `table.delete(key).await?;`                                                                                                                    |
| `table.store(&x).when(c).run().await?`                        | `table.put(&x).when(c).await?`                                                                                                                 |
| `table.update(k, e).when(c).run().await?`                     | `table.update(k, e).when(c).await?`                                                                                                            |
| `use keygrip::expression::Expression`                         | `use keygrip::Expression`                                                                                                                      |
| `.string(":s", v)` / `.number(":n", v)` / `.boolean(":b", v)` | `.value(":s", &v)` / `.value(":n", &v)` / `.value(":b", &v)`                                                                                   |
| `.value(":x", item::value(&x)?)`                              | `.value(":x", &x)`                                                                                                                             |
| `.value(":l", attr::list(vec![attr::s(s)]))`                  | `.value(":l", &vec![s])` (a list, `L`)                                                                                                         |
| `Transaction::new().put(&t, &x)?.when(c).label("a")`          | `Transaction::new().add(t.put(&x).when(c)).label("a")`                                                                                         |
| `run(&client).await` → `Err(e) if e.failed("a")`              | `run(&client).await?` → `outcome.rejected("a")`; **check `committed()`**                                                                       |
| `occ::retry(n, conflict, …)`                                  | a retry loop in your crate around a write that resolves to `false`                                                                             |
| raw `get_item` + `item::from` for another item shape          | a hand-written `impl Schema` for that shape and its own `Entity` on the same table                                                             |
| `request::unavailable` for your own errors                    | your own error type                                                                                                                            |
| `Error::Unavailable` on (de)serialization failures            | `Error::Invalid`                                                                                                                               |
| `match` on `Error::Conflict(_)` (e.g. → HTTP 409)             | remove the arm; conflicts are the `bool`/`Outcome` at the call site. Add an `Error::Invalid(_)` arm                                            |

## [0.3.3](https://github.com/insd47/keygrip/tree/df15df7) - 2026-07-19

### Added

- `Entity::merge` for conditional whole-value updates. It preserves selected attributes with `if_not_exists` and
  returns the stored item (`ALL_NEW`) through `fetch`. Unlike the full replacement of `store`, a merge keeps attributes
  absent from the new value.
