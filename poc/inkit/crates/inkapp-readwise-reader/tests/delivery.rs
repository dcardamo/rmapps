//! Deferred-write delivery: flush drains the write queue through a transport,
//! retrying transient failures and surfacing permanent ones.

use std::sync::Arc;

use inkapp_core::connector::Connector;
use inkapp_readwise_reader::{Readwise, ScriptedTransport, MAX_ATTEMPTS};

#[tokio::test]
async fn refresh_is_ok_and_keeps_queue_warm() {
    let rw = Readwise::fake();
    assert!(rw.refresh().await.is_ok());
    assert!(rw.refresh().await.is_ok(), "refresh is idempotent");
    assert_eq!(rw.queue().len(), 2, "fake cassette: two articles");
}

#[tokio::test]
async fn transient_failure_is_retried_then_delivered() {
    let transport = Arc::new(ScriptedTransport::failing(2)); // fail twice, then succeed
    let rw = Readwise::fake().with_transport(transport.clone());
    let id = rw.queue()[0].id.clone();
    rw.archive(&id);

    rw.flush().await; // attempt 1 -> fail, requeued
    rw.flush().await; // attempt 2 -> fail, requeued
    rw.flush().await; // attempt 3 -> succeeds, delivered

    assert_eq!(
        transport.delivered(),
        1,
        "delivered exactly once after retries"
    );
    assert!(rw.failed_writes().is_empty(), "no permanent failures");
}

#[tokio::test]
async fn permanent_failure_surfaces_in_failed_writes() {
    let transport = Arc::new(ScriptedTransport::always_failing());
    let rw = Readwise::fake().with_transport(transport);
    let id = rw.queue()[0].id.clone();
    rw.archive(&id);

    for _ in 0..MAX_ATTEMPTS {
        rw.flush().await;
    }

    assert_eq!(
        rw.failed_writes().len(),
        1,
        "permanently-failed write surfaces"
    );
}
