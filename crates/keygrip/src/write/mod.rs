//! Single-item writes: [`Put`], [`Update`], and [`Delete`].
//!
//! Every write is a value built from an [`Entity`](crate::Entity), optionally
//! guarded by one [`Condition`], and executed by awaiting it. A rejected
//! condition is a value, not an error: the write resolves to `false`, and
//! domain code decides whether that means a retry, a fallback read, or a
//! conflict. The same values assemble into a
//! [`Transaction`](crate::Transaction).

mod condition;
mod delete;
mod put;
mod update;

pub use condition::Condition;
pub use delete::Delete;
pub use put::Put;
pub use update::Update;

use crate::binding::Bindings;
use crate::{request, Error, Expression, Result};
use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_dynamodb::types::AttributeValue;
use condition::Slot;
use std::collections::HashMap;
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;

/// The boxed future a write resolves through when awaited directly.
type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// A write's condition merged with every placeholder the write binds —
/// the part shared by single requests and transaction steps.
struct Clause {
    condition: Option<String>,
    names: Option<HashMap<String, String>>,
    values: Option<HashMap<String, AttributeValue>>,
}

/// Merges an optional update expression with an optional condition,
/// rejecting placeholders bound by both.
fn clause(update: Option<Bindings>, condition: Option<Expression>) -> Result<(Option<String>, Clause)> {
    let condition = condition.map(Expression::compile).transpose()?;

    if let (Some(update), Some(condition)) = (&update, &condition) {
        if let Some(collision) = update.collision(condition) {
            return Err(invalid(collision));
        }
    }

    let (statement, mut names, mut values) = match update {
        Some(update) => (Some(update.statement), update.names, update.values),
        None => (None, HashMap::new(), HashMap::new()),
    };
    let condition = condition.map(|condition| {
        names.extend(condition.names);
        values.extend(condition.values);
        condition.statement
    });

    Ok((
        statement,
        Clause {
            condition,
            names: present(names),
            values: present(values),
        },
    ))
}

/// Maps a single-item response: `None` when the condition was rejected.
fn applied<O, E, R>(result: std::result::Result<O, SdkError<E, R>>) -> Result<Option<O>>
where
    E: ProvideErrorMetadata,
    SdkError<E, R>: Display,
{
    match result {
        Ok(output) => Ok(Some(output)),
        Err(error) if request::conditional(&error) => Ok(None),
        Err(error) => Err(request::unavailable(error)),
    }
}

fn invalid(detail: impl Into<String>) -> Error {
    Error::Invalid(detail.into())
}

fn present<K, V>(values: HashMap<K, V>) -> Option<HashMap<K, V>> {
    (!values.is_empty()).then_some(values)
}
