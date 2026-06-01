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
//! ── TWO TESTS LIVE HERE ──────────────────────────────────────────────────────────────────
//! The notification socket only pushes a frame when SOME OTHER client commits with the root
//! PUT flag `broadcast: true`. The normal rm-cloud `commit`/`put`/`put_content_only` path uses
//! `broadcast: false` (the daemon must not self-trigger), so a change made on the SAME
//! subscription does NOT notify it.
//!
//! - `live_push_subscription_stays_healthy` proves the subscription handshake + a healthy
//!   socket using a single connection's own broadcast:false write (no self-wakeup expected).
//! - `live_push_delivers_on_broadcast` proves actual cross-connection PUSH DELIVERY: a second
//!   connection makes a `broadcast: true` change (via `put_content_only_broadcast`) and the
//!   subscribed connection must receive a frame. See its doc-comment for the same-device-token
//!   uncertainty.
//!
//! `live_push_subscription_stays_healthy` asserts what a single connection legitimately can:
//!   1. The subscription handshake succeeds (socket connects + authenticates).
//!   2. The socket stays healthy: a bounded read returns either no message (Elapsed — the
//!      EXPECTED outcome, since our own broadcast:false write does not notify us) or a benign
//!      Ping/Pong frame. An error frame within the window is a FAILURE (a dead/refused socket).
//! Cross-connection end-to-end push delivery is the job of `live_push_delivers_on_broadcast`
//! (a second connection broadcasts; this one must receive a frame). The manual Tier B check
//! (a SEPARATE registered device, distinct token) remains the fallback to disambiguate the
//! same-device-token uncertainty. We never fake a wakeup.

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

/// Tier A automated PUSH-DELIVERY proof using two connections that share one device token.
///
/// Connection A subscribes to the notification websocket and parks on the next frame.
/// Connection B (a separate `Client`, SAME device token) seeds a doc and then makes a
/// BROADCASTING change (`put_content_only_broadcast`, which routes through the root-PUT
/// `broadcast: true` flag). If the reMarkable cloud cross-notifies, A receives a wakeup frame.
///
/// ── KNOWN UNCERTAINTY (read before interpreting a timeout) ──────────────────────────────────
/// A and B use the SAME device token. The cloud MAY key broadcast delivery by device identity
/// and decline to cross-notify two sessions of the same device token (real devices each have a
/// DISTINCT token). If so, this test will TIME OUT even though the broadcast mechanism is sound
/// between two distinct devices. We deliberately do NOT weaken the assertion to pass on timeout:
/// the entire point is to prove delivery, so no-delivery is a FAILURE — but the panic message
/// names the same-token hypothesis so a timeout can be disambiguated against a real delivery
/// regression. The controller runs this live (once the account's 429 clears) to learn whether
/// same-token cross-notify works; if it does not, the manual Tier B check (a second registered
/// device) is the disambiguating fallback.
#[tokio::test]
#[ignore = "hits the live reMarkable notification websocket; needs RM_CLOUD_DEVICE_TOKEN"]
async fn live_push_delivers_on_broadcast() {
    let token = match std::env::var("RM_CLOUD_DEVICE_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            eprintln!("skipping live push-delivery test: RM_CLOUD_DEVICE_TOKEN unset");
            return;
        }
    };

    // Two independent clients, SAME device token (see the uncertainty note above).
    let client_a = Client::from_device_token(Config::from_env(), token.clone());
    let client_b = Client::from_device_token(Config::from_env(), token);

    // Scratch isolation folder created by B under the shared rmrs-test root.
    let run_id = Uuid::new_v4().to_string();
    let base = get_or_create_folder(&client_b, ROOT_TEST_DIR, "")
        .await
        .expect("get/create test root folder");
    let run_folder = client_b.mkdir(&run_id, &base).await.expect("mk run folder");

    let result = push_delivery_body(&client_a, &client_b, &run_folder).await;

    match result {
        Ok(()) => {
            client_b
                .rm(&run_folder)
                .await
                .expect("cleanup run folder on success");
        }
        Err(e) => {
            eprintln!(
                "live_push_delivers_on_broadcast FAILED; leaving \
                 {ROOT_TEST_DIR}/{run_id} for inspection: {e}"
            );
            panic!("live push-delivery test failed: {e}");
        }
    }
}

async fn push_delivery_body(client_a: &Client, client_b: &Client, run_folder: &str) -> Result<()> {
    // 1. A subscribes FIRST so the socket is live before B broadcasts.
    let mut stream = client_a.notifications_subscribe().await?;
    eprintln!("[watch_live] A handshake OK — notification websocket connected");

    // 2. B seeds a doc (broadcast:false `put` — must NOT be the frame we measure).
    let id = Uuid::new_v4().to_string();
    let meta = Metadata {
        visible_name: "watch-live-delivery".into(),
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
            (format!("{id}.pdf"), b"%PDF-1.4 seed".to_vec()),
        ],
    };
    client_b.put(df).await?;
    eprintln!("[watch_live] B seeded doc {id} (broadcast:false)");

    // 3. Park A on the NEXT frame BEFORE B broadcasts. Spawn the read task, then sleep briefly so
    //    the await is genuinely pending, then broadcast.
    let reader =
        tokio::spawn(
            async move { tokio::time::timeout(Duration::from_secs(30), stream.next()).await },
        );
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 4. B makes a BROADCASTING change to the seeded doc (root PUT broadcast:true).
    client_b
        .put_content_only_broadcast(&id, b"%PDF-1.4 bumped".to_vec())
        .await?;
    eprintln!("[watch_live] B broadcast a content change to {id} (broadcast:true)");

    // 5. Await the parked read. A frame within 30s proves delivery; timeout/None/Err is failure.
    let outcome = reader.await.expect("reader task panicked");
    match outcome {
        Ok(Some(Ok(msg))) => {
            eprintln!("[watch_live] A RECEIVED a push frame after B's broadcast: {msg:?}");
        }
        Ok(Some(Err(e))) => {
            // Cleanup the doc before bubbling the error (folder is left by the caller on Err).
            let _ = client_b.rm(&id).await;
            return Err(rm_cloud::Error::Http(format!(
                "notification socket errored awaiting the broadcast frame: {e}"
            )));
        }
        Ok(None) => {
            let _ = client_b.rm(&id).await;
            return Err(rm_cloud::Error::Http(
                "notification socket closed before delivering the broadcast frame".into(),
            ));
        }
        Err(_elapsed) => {
            let _ = client_b.rm(&id).await;
            return Err(rm_cloud::Error::Http(
                "No push frame received within 30s. This may mean the cloud does not \
                 cross-notify two connections sharing one device token (would need a second \
                 registered device token), OR a real delivery failure. Run the manual Tier B \
                 check (separate device) to disambiguate."
                    .into(),
            ));
        }
    }

    // 6. Clean up the doc; the caller removes the run folder on overall success.
    client_b.rm(&id).await?;
    Ok(())
}
