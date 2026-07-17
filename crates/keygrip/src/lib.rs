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
//! # Feature flags
//!
//! - `dynamodb` *(default)* — the AWS SDK-backed [`Entity`], [`Query`], and the
//!   extension toolkit ([`attr`], [`expression`], [`item`], [`occ`],
//!   [`request`], [`transaction`], [`write`](mod@write)).
//! - Without default features, only the schema vocabulary ([`Schema`],
//!   [`Parts`], [`Index`], [`KeyPart`]) and the derive macro remain — for
//!   model-only crates that must not compile the AWS SDK.

extern crate self as keygrip;

#[cfg(feature = "dynamodb")]
pub mod attr;
#[cfg(feature = "dynamodb")]
mod binding;
#[cfg(feature = "dynamodb")]
mod entity;
#[cfg(feature = "dynamodb")]
mod error;
#[cfg(feature = "dynamodb")]
pub mod expression;
#[cfg(feature = "dynamodb")]
pub mod item;
#[cfg(feature = "dynamodb")]
mod key;
#[cfg(feature = "dynamodb")]
pub mod occ;
#[cfg(feature = "dynamodb")]
mod query;
#[cfg(feature = "dynamodb")]
pub mod request;
mod schema;
#[cfg(feature = "dynamodb")]
pub mod transaction;
#[cfg(feature = "dynamodb")]
mod types;
#[cfg(feature = "dynamodb")]
pub mod write;

#[cfg(feature = "dynamodb")]
pub use entity::Entity;
#[cfg(feature = "dynamodb")]
pub use error::{Error, Result};
pub use keygrip_derive::Schema;
#[cfg(feature = "dynamodb")]
pub use query::Query;
pub use schema::{Index, Key, KeyPart, Parts, Schema};
#[cfg(feature = "dynamodb")]
pub use types::{Cursor, Page};
