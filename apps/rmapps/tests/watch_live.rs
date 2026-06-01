//! Tier A gated live e2e — the PUSH half (notification websocket delivery).
//!
//! Gated by `RM_CLOUD_DEVICE_TOKEN` and `#[ignore]` so it never runs by default. Talks to
//! PRODUCTION (do NOT set `RM_CLOUD_HOST`). All writes happen inside `rmrs-test/<run-id>`;
//! the run folder is deleted on success and left on failure for inspection.
//!
//! Run live with:
//!   RM_CLOUD_DEVICE_TOKEN="$(jq -r .device_token ~/.config/rmapps/auth.json)" \
//!     cargo test -p rmapps --test watch_live -- --ignored --nocapture
//!
//! ── WHAT THIS TEST CAN AND CANNOT ASSERT ────────────────────────────────────────────────
//! The notification socket only pushes a frame when SOME OTHER client commits with the root
//! PUT flag `broadcast: true`. The rm-cloud `commit`/`put`/`put_content_only` path hardcodes
//! `broadcast: false` (crates/rm-cloud/src/client.rs) and exposes NO public override. So a
//! change made by THIS test (or by the daemon itself) does NOT notify our own subscription —
//! that is intentional: the daemon must not self-trigger.
//!
//! Therefore this AUTOMATED test cannot trigger a wakeup from its own writes. It asserts what
//! it legitimately can:
//!   1. The subscription handshake succeeds (socket connects + authenticates).
//!   2. The socket stays healthy: a bounded read returns either no message (Elapsed — the
//!      EXPECTED outcome, since our own broadcast:false write does not notify us) or a benign
//!      Ping/Pong frame. An error frame within the window is a FAILURE (a dead/refused socket).
//! Genuine end-to-end push delivery — an external reMarkable device or another broadcasting
//! client mutating the account and our socket receiving the Wakeup — is covered by the manual
//! Tier B verification, NOT by this automated test. We do not fake a wakeup.

use futures_util::StreamExt;
use rm_cloud::{Client, Config, DocFiles, Metadata, Result};
use std::time::Duration;
use uuid::Uuid;

const ROOT_TEST_DIR: &str = "rmrs-test";

/// Build a device-token client from the env (PRODUCTION), or print a skip notice and `None`.
fn client_or_skip() -> Option<Client> {
    match std::env::var("RM_CLOUD_DEVICE_TOKEN") {
        Ok(t) if !t.is_empty() => Some(Client::from_device_token(Config::from_env(), t)),
        _ => {
            eprintln!("skipping live push test: RM_CLOUD_DEVICE_TOKEN unset");
            None
        }
    }
}

/// Find folder `name` under `parent`, creating it if absent. Returns its id.
async fn get_or_create_folder(client: &Client, name: &str, parent: &str) -> Result<String> {
    if let Some(e) = client
        .ls(parent)
        .await?
        .into_iter()
        .find(|e| e.is_folder && e.name == name)
    {
        return Ok(e.id);
    }
    client.mkdir(name, parent).await
}

#[tokio::test]
#[ignore = "hits the live reMarkable notification websocket; needs RM_CLOUD_DEVICE_TOKEN"]
async fn live_push_subscription_stays_healthy() {
    let Some(client) = client_or_skip() else {
        return;
    };

    // Unique isolation folder under the shared rmrs-test root.
    let run_id = Uuid::new_v4().to_string();
    let base = get_or_create_folder(&client, ROOT_TEST_DIR, "")
        .await
        .expect("get/create test root folder");
    let run_folder = client.mkdir(&run_id, &base).await.expect("mk run folder");

    // Run the body so we can implement leave-on-failure: clean up only on Ok.
    let result = push_body(&client, &run_folder).await;

    match result {
        Ok(()) => {
            client
                .rm(&run_folder)
                .await
                .expect("cleanup run folder on success");
        }
        Err(e) => {
            eprintln!(
                "live_push_subscription_stays_healthy FAILED; leaving \
                 {ROOT_TEST_DIR}/{run_id} for inspection: {e}"
            );
            panic!("live push test failed: {e}");
        }
    }
}

async fn push_body(client: &Client, run_folder: &str) -> Result<()> {
    // 1. Subscribe FIRST so the socket is live before we mutate the account.
    let mut stream = client.notifications_subscribe().await?;
    eprintln!("[watch_live] handshake OK — notification websocket connected");

    // 2. Make a change inside the scratch folder. This is broadcast:false (no public override),
    //    so our OWN subscription will NOT be notified — see the module-level note. We still
    //    perform the write so the test exercises a realistic concurrent-write scenario and so
    //    the scratch folder is genuinely touched.
    let id = Uuid::new_v4().to_string();
    let meta = Metadata {
        visible_name: "watch-live-push".into(),
        doc_type: "DocumentType".into(),
        parent: run_folder.into(),
        last_modified: "0".into(),
        deleted: false,
        extra: Default::default(),
    };
    let df = DocFiles {
        id: id.clone(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), b"{}".to_vec()),
            (format!("{id}.pdf"), b"%PDF-1.4 test".to_vec()),
        ],
    };
    client.put(df).await?;
    client
        .put_content_only(&id, b"%PDF-1.4 updated".to_vec())
        .await?;

    // 3. The socket must stay healthy. Because our writes are broadcast:false, the EXPECTED
    //    outcome is Elapsed (no frame). A benign Ping/Pong/Close-less keepalive is also fine.
    //    Only an Err frame (a broken/refused socket) within the window is a failure.
    match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
        Err(_elapsed) => {
            // No frame in 5s — exactly what we expect for our own broadcast:false write.
            eprintln!(
                "[watch_live] no wakeup within 5s (EXPECTED: our own commits use \
                 broadcast:false, so they do not notify our subscription). Socket healthy."
            );
        }
        Ok(Some(Ok(msg))) => {
            // A frame DID arrive. That is not an error (the account may have changed for other
            // reasons, e.g. another device). We only reject explicit error frames below.
            eprintln!("[watch_live] received a frame while subscribed: {msg:?}");
        }
        Ok(Some(Err(e))) => {
            return Err(rm_cloud::Error::Http(format!(
                "notification socket errored while subscribed (not a clean keepalive): {e}"
            )));
        }
        Ok(None) => {
            return Err(rm_cloud::Error::Http(
                "notification socket closed unexpectedly while subscribed".into(),
            ));
        }
    }

    // 4. Clean up the doc; the caller removes the run folder on overall success.
    client.rm(&id).await?;
    Ok(())
}
