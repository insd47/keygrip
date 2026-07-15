//! Typed, key-centric DynamoDB access for Rust.
//!
//! `keygrip` derives a table's key schema from its storage model and gives you
//! a small typed [`Handle`] for the operations DynamoDB is actually good at:
//! key-based CRUD, batch reads, scans, and partition queries.
//!
//! ```no_run
//! use aws_sdk_dynamodb::Client;
//! use keygrip::{Entity, Handle, Result};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize, Entity)]
//! #[entity(pk(contest_id), sk(id))]
//! #[serde(rename_all = "camelCase")]
//! struct UserTable {
//!     contest_id: String,
//!     id: String,
//!     name: String,
//! }
//!
//! async fn users(client: Client) -> Result<Vec<UserTable>> {
//!     let users = Handle::<UserTable>::new(client, "Users");
//!
//!     users.query("contest").newest().all().await
//! }
//! ```
//!
//! # Feature flags
//!
//! - `dynamodb` *(default)* — the AWS SDK-backed [`Handle`], [`Query`], and the
//!   extension toolkit ([`attr`], [`item`], [`occ`], [`request`]).
//! - Without default features, only the entity vocabulary ([`Entity`],
//!   [`Parts`], [`Index`], [`KeyPart`]) and the derive macro remain — for
//!   model-only crates that must not compile the AWS SDK.

extern crate self as keygrip;

#[cfg(feature = "dynamodb")]
pub mod attr;
mod entity;
#[cfg(feature = "dynamodb")]
mod error;
#[cfg(feature = "dynamodb")]
mod handle;
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
#[cfg(feature = "dynamodb")]
mod types;

pub use entity::{Entity, Index, Key, KeyPart, Parts};
#[cfg(feature = "dynamodb")]
pub use error::{Error, Result};
#[cfg(feature = "dynamodb")]
pub use handle::Handle;
pub use keygrip_derive::Entity;
#[cfg(feature = "dynamodb")]
pub use query::Query;
#[cfg(feature = "dynamodb")]
pub use types::{Cursor, Page};
