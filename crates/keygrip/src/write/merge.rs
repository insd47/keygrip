use super::{Condition, Pending};
use crate::transaction::Step;
use crate::{Entity, Result, Schema, Update};
use std::future::{Future, IntoFuture};

/// An update that sets every serialized non-key attribute of a whole value.
///
/// Built by [`Entity::merge`]. Attributes absent from the value are not
/// removed, unlike [`put`](Entity::put), so use it only when every write
/// preserves the same field set. Awaiting it resolves like an [`Update`]:
/// `true` when applied, `false` when its [`Condition`] was rejected.
#[must_use = "a write does nothing until it is awaited"]
pub struct Merge<'a, E: Schema> {
    update: Update<'a, E>,
}

impl<'a, E: Schema> Merge<'a, E> {
    pub(crate) fn new(entity: &'a Entity<E>, value: &'a E) -> Self {
        Self {
            update: Update::merge(entity, value),
        }
    }

    /// Writes `attribute` only when the stored item does not already have it
    /// (`if_not_exists`).
    ///
    /// An attribute the value does not serialize fails with
    /// [`Error::Invalid`](crate::Error::Invalid) when the write runs.
    pub fn keep(self, attribute: impl Into<String>) -> Self {
        Self {
            update: self.update.keep(attribute.into()),
        }
    }

    /// Attaches the condition that must hold for the merge to apply.
    ///
    /// At most one condition may be attached; a second one fails with
    /// [`Error::Invalid`](crate::Error::Invalid) when the write runs.
    pub fn when(self, condition: impl Into<Condition>) -> Self {
        Self {
            update: self.update.when(condition),
        }
    }

    /// Applies the merge, resolving to `false` when its condition is rejected.
    ///
    /// Awaiting the `Merge` directly does the same.
    pub fn run(self) -> impl Future<Output = Result<bool>> + Send + 'a {
        self.update.run()
    }

    /// Applies the merge and returns the stored item (`ALL_NEW`), or `None`
    /// when its condition is rejected.
    pub fn fetch(self) -> impl Future<Output = Result<Option<E>>> + Send + 'a {
        self.update.fetch()
    }
}

impl<'a, E: Schema> IntoFuture for Merge<'a, E> {
    type Output = Result<bool>;
    type IntoFuture = Pending<'a, bool>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

impl<E: Schema> From<Merge<'_, E>> for Step {
    fn from(write: Merge<'_, E>) -> Self {
        write.update.into()
    }
}
