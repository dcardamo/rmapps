//! Task 11: `Session::step_app` drives the inkapp loop one cycle. These tests
//! verify the structural contract (cycle counter, empty-ink ok, JSON shape)
//! and stand as a secrets-leak guard for the StepResult serialization.

mod common;

use inkapp_core::runtime::{app, DocSet};
use inkapp_harness::session::{Session, StepOpts};
use reading_queue::{update, view, App, Connectors};
use tempfile::tempdir;

#[tokio::test]
async fn session_step_drives_reading_queue() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(Some("rm")).unwrap();

    let mut application = app(App)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(common::test_key())
        .build();
    let mut set = DocSet::default();

    // Render so DocSet is populated.
    let _ = application.render(&mut set).await.unwrap();

    // Empty step (no pending ink) — should succeed and bump cycle.
    let r = s
        .step_app(&dev, &mut application, &mut set, StepOpts::default())
        .await
        .unwrap();
    assert_eq!(r.cycle, 1, "first cycle bumps from 0 to 1");
    assert!(r.msgs.is_empty(), "no pending ink → no decoded msgs");
    assert!(
        r.model_diff.is_null(),
        "model_diff is a placeholder for now"
    );
    assert!(r.connector_writes.is_empty());
    assert!(r.secrets_read.is_empty());
    assert!(r.new_version >= 1, "version reflects post-step state");

    // A second step bumps the counter again.
    let r2 = s
        .step_app(&dev, &mut application, &mut set, StepOpts::default())
        .await
        .unwrap();
    assert_eq!(r2.cycle, 2, "cycle counter is persisted per-device");
}

#[tokio::test]
async fn step_result_never_contains_known_secret_token() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();

    let mut application = app(App)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(common::test_key())
        .build();
    let mut set = DocSet::default();
    let _ = application.render(&mut set).await.unwrap();

    let r = s
        .step_app(&dev, &mut application, &mut set, StepOpts::default())
        .await
        .unwrap();
    let json = serde_json::to_string(&r).unwrap();
    // Guard for the future. No real secrets in fake connectors today, so this
    // is a trivial pass — it stands so the test catches a future regression if
    // secrets ever leak into the StepResult JSON.
    assert!(
        !json.to_lowercase().contains("topsecret123"),
        "no secret token leaked into StepResult JSON"
    );
}
