//! `SingleFlight` — collapse concurrent identical async work into one execution.
//! The doc's connector model wants a refresh stampede to become a single network
//! call; every connector can reuse this rather than reinventing it.

use std::future::Future;
use std::sync::Mutex;

use futures::future::{BoxFuture, FutureExt, Shared};

/// Collapses concurrent `run` calls into a single shared execution. The first
/// caller (or whichever locks the slot first) creates the future; concurrent
/// callers join it. Once it completes, the next call starts fresh.
pub struct SingleFlight<T: Clone> {
    slot: Mutex<Option<Shared<BoxFuture<'static, T>>>>,
}

impl<T: Clone> Default for SingleFlight<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }
}

impl<T: Clone + Send + 'static> SingleFlight<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `make`'s future, sharing an in-flight execution with concurrent callers.
    pub async fn run<F, Fut>(&self, make: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T> + Send + 'static,
    {
        let shared = {
            let mut slot = self.slot.lock().unwrap();
            match slot.as_ref() {
                // A flight is in progress (not yet completed) — join it.
                Some(s) if s.peek().is_none() => s.clone(),
                // No flight, or the previous one already finished — start fresh.
                _ => {
                    let s = make().boxed().shared();
                    *slot = Some(s.clone());
                    s
                }
            }
        };
        shared.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn collapses_concurrent_calls_to_one_execution() {
        let sf = SingleFlight::<usize>::new();
        let calls = Arc::new(AtomicUsize::new(0));

        // Two run() futures are polled together by join!. The first to be polled
        // creates the shared future (incrementing `calls`) and yields once; the
        // second locks the slot mid-flight (peek == None) and joins it — so the
        // underlying closure runs exactly once.
        let c1 = calls.clone();
        let c2 = calls.clone();
        let (a, b) = tokio::join!(
            sf.run(|| async move {
                c1.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                42usize
            }),
            sf.run(|| async move {
                c2.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                42usize
            }),
        );

        assert_eq!(a, 42);
        assert_eq!(b, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one underlying execution");
    }

    #[tokio::test]
    async fn fresh_flight_after_completion() {
        let sf = SingleFlight::<usize>::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let c = calls.clone();
        let first = sf
            .run(|| async move {
                c.fetch_add(1, Ordering::SeqCst);
                1usize
            })
            .await;
        assert_eq!(first, 1);

        // First flight has completed; a second run starts a new execution.
        let c = calls.clone();
        let second = sf
            .run(|| async move {
                c.fetch_add(1, Ordering::SeqCst);
                2usize
            })
            .await;
        assert_eq!(second, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "two separate executions");
    }
}
