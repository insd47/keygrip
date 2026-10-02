//! Typed, key-centric DynamoDB access for Rust.
//!
//! `keygrip` derives a table's key schema from its storage model and gives you
//! a live typed [`Entity`] for the operations DynamoDB is actually good at:
//! key-based CRUD, batch reads, scans, and partition queries.
//!
//! ```no_run
//! use aws_sdk_dynamodb::Client;
//! use keygrip::{Entity, Result, Schema};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize, Schema)]
//! #[entity(pk(contest_id), sk(id))]
//! #[serde(rename_all = "camelCase")]
//! struct UserTable {
//!     contest_id: String,
//!     id: String,
//!     name: String,
//! }
//!
//! async fn users(client: Client) -> Result<Vec<UserTable>> {
//!     let users = Entity::<UserTable>::new(&client, "Users");
//!
//!     users.query("contest").newest().all().await
//! }
//! ```
//!
//! # Writes
//!
//! Writes are values: [`Entity::put`], [`update`](Entity::update),
//! [`merge`](Entity::merge), and [`delete`](Entity::delete) return a [`Put`],
//! [`Update`], [`Merge`], or [`Delete`] that runs when awaited, optionally guarded by
//! one [`Condition`]. A rejected condition resolves to `false` rather than an
//! error, and the same values assemble into an atomic [`Transaction`]:
//!
//! ```no_run
//! # use keygrip::{Entity, Schema};
//! # use serde::{Deserialize, Serialize};
//! # #[derive(Serialize, Deserialize, Schema)]
//! # #[entity(pk(id))]
//! # struct UserTable { id: String, exited: bool }
//! use keygrip::{Condition, Expression, Result};
//!
//! async fn exit(users: &Entity<UserTable>, id: &str) -> Result<bool> {
//!     users
//!         .update(id, Expression::new("SET exited = :exited").value(":exited", &true))
//!         .when(Condition::exists())
//!         .await
//! }
//! ```

extern crate self as keygrip;

mod binding;
mod entity;
mod error;
mod expression;
mod item;
mod key;
mod query;
mod request;
mod schema;
mod transaction;
mod types;
mod write;

pub use entity::Entity;
pub use error::{Error, Result};
pub use expression::Expression;
pub use keygrip_derive::Schema;
pub use query::Query;
pub use schema::{Index, Key, KeyPart, Parts, Schema, SortSpace};
pub use transaction::{Outcome, Transaction};
pub use types::{Cursor, Page};
pub use write::{Condition, Delete, Merge, Put, Update};
