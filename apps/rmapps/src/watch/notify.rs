//! Wakeup sources for the reactor. A Wakeup is only a signal; the diff is the
//! source of truth for what changed, so all sources are interchangeable.
// Consumed by the daemon wiring in Task 7/8; nothing references these yet.
#![allow(dead_code)]
use async_trait::async_trait;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

#[derive(Clone, Copy, Debug)]
pub struct Wakeup;

#[async_trait]
pub trait NotificationSource: Send {
    /// Resolve when the account may have changed. Never returns an error: a source
    /// that dies should reconnect internally and keep yielding (poll fallback covers gaps).
    async fn next_wakeup(&mut self) -> Wakeup;
}

/// Periodic safety-net source (also the `--poll-only` mode).
pub struct PollSource {
    interval: tokio::time::Interval,
}
impl PollSource {
    pub fn new(period: Duration) -> Self {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self { interval }
    }
}
#[async_trait]
impl NotificationSource for PollSource {
    async fn next_wakeup(&mut self) -> Wakeup {
        self.interval.tick().await;
        Wakeup
    }
}

/// Test source driven by a channel.
pub struct FakeSource {
    rx: Receiver<Wakeup>,
}
impl FakeSource {
    pub fn new(rx: Receiver<Wakeup>) -> Self {
        Self { rx }
    }
}
#[async_trait]
impl NotificationSource for FakeSource {
    async fn next_wakeup(&mut self) -> Wakeup {
        self.rx.recv().await.unwrap_or(Wakeup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_source_delivers_pushed_wakeups() {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let mut src = FakeSource::new(rx);
        tx.send(Wakeup).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), src.next_wakeup())
            .await
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn poll_source_ticks() {
        let mut src = PollSource::new(Duration::from_millis(50));
        // First tick fires immediately (tokio interval semantics).
        src.next_wakeup().await;
        // Second tick after advancing virtual time.
        tokio::time::advance(Duration::from_millis(60)).await;
        let _ = tokio::time::timeout(Duration::from_secs(1), src.next_wakeup())
            .await
            .unwrap();
    }
}
