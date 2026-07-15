//! Typed, key-centric DynamoDB access for Rust.

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
