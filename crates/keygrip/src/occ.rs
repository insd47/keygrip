//! Optimistic-concurrency retry loop, for extension code implementing
//! read-modify-write invariants with version-conditioned updates.

use std::future::Future;

/// Outcome of one optimistic attempt.
pub enum Attempt<T> {
    /// The conditional write went through; stop with this value.
    Done(T),
    /// The version condition was rejected; re-read and try again.
    Retry,
}

/// Runs `attempt` until it completes, retrying rejected attempts at most
/// `attempts` times before failing with `conflict`.
///
/// Real errors returned by `attempt` propagate immediately — only
/// [`Attempt::Retry`] consumes an attempt.
pub async fn retry<T, E, F, Fut>(attempts: usize, conflict: E, mut attempt: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Attempt<T>, E>>,
{
    for _ in 0..attempts {
        match attempt().await? {
            Attempt::Done(value) => return Ok(value),
            Attempt::Retry => {}
        }
    }

    Err(conflict)
}

#[cfg(test)]
mod tests {
    use super::{retry, Attempt};
    use crate::Error;
    use std::cell::Cell;

    #[tokio::test]
    async fn retries_only_conflicted_attempts() {
        let attempts = Cell::new(0);
        let result = retry(3, Error::Conflict("conflict"), || async {
            attempts.set(attempts.get() + 1);

            if attempts.get() < 3 {
                Ok(Attempt::Retry)
            } else {
                Ok(Attempt::Done("done"))
            }
        })
        .await;

        assert_eq!(result.unwrap(), "done");
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn returns_conflict_after_the_attempt_limit() {
        let result = retry::<(), _, _, _>(2, Error::Conflict("conflict"), || async {
            Ok(Attempt::Retry)
        })
        .await;

        assert!(matches!(result, Err(Error::Conflict("conflict"))));
    }
}
