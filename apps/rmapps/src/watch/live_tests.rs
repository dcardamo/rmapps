//! Tier A gated live e2e — the REACTOR half (the full reconcile pipeline against the REAL
//! reMarkable cloud).
//!
//! This lives INSIDE the crate (not in `tests/`) because it must call the private
//! `super::reconcile_pass`, which an external integration test cannot reach (rmapps is a
//! binary crate). It mirrors `reactor_tests.rs` but points the app `Cloud` at PRODUCTION
//! (built from `RM_CLOUD_DEVICE_TOKEN`, with `RM_CLOUD_HOST` left UNSET) and operates inside
//! a unique `rmrs-test/<run-id>/Books` scratch folder.
//!
//! Gated by `RM_CLOUD_DEVICE_TOKEN` and `#[ignore]`. The scratch folder is removed on success
//! and left on failure for inspection.
//!
//! Run live with:
//!   RM_CLOUD_DEVICE_TOKEN="$(jq -r .device_token ~/.config/rmapps/auth.json)" \
//!     cargo test -p rmapps live_reactor -- --ignored --nocapture
//!
//! Runtime/seam constraints (same as reactor_tests.rs):
//!   * The app `Cloud` is SYNCHRONOUS and owns its own tokio runtime (it `block_on`s
//!     internally), so this MUST be a plain `#[test]`, never `#[tokio::test]`.
//!   * We seed the cloud with a raw async `rm_cloud::Client` driven on a dedicated,
//!     kept-alive runtime.
//!   * We do NOT set `RM_CLOUD_HOST` — the app Cloud talks to production via
//!     `Config::from_env()` (unset host = production). We DO set `RMAPPS_WATCH_STATE` to a
//!     temp file so reconcile_pass never touches the real daemon state.

use std::time::{Duration, Instant};

use rm_cloud::{Client, Config as CloudConfig, DocFiles, Metadata};
use uuid::Uuid;

use crate::config::{Config, WatchAction};
use crate::watch::debounce::Debouncer;
use crate::watch::state::WatchState;

const ROOT_TEST_DIR: &str = "rmrs-test";

/// Build a PDF-backed document under `parent` (folder id) with the given visible name.
fn doc_with_pdf(id: &str, name: &str, parent: &str, pdf: &[u8]) -> DocFiles {
    let meta = Metadata {
        visible_name: name.into(),
        doc_type: "DocumentType".into(),
        parent: parent.into(),
        last_modified: "0".into(),
        deleted: false,
        extra: Default::default(),
    };
    DocFiles {
        id: id.into(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), b"{}".to_vec()),
            (format!("{id}.pdf"), pdf.to_vec()),
        ],
    }
}

/// A minimal rmapps Config with a `[digest]` section (so the digest suffixes populate the
/// self-write exclude list) and one Digest watch rule on `watch_path`. Built from TOML so the
/// rmdigest defaults (digest_suffix " — Digest", annotated_suffix " — Annotated") apply.
fn config_with_watch(watch_path: &str) -> Config {
    let toml = format!(
        r#"
[digest]
device = "test-device"
watched_paths = ["{watch_path}"]

[[watch]]
path = "{watch_path}"
action = "digest"
debounce = "30s"
"#
    );
    let cfg: Config = toml::from_str(&toml).expect("config TOML parses");
    cfg.validate().expect("config validates");
    cfg
}

/// Find folder `name` under `parent`, creating it if absent. Returns its id.
async fn get_or_create_folder(client: &Client, name: &str, parent: &str) -> rm_cloud::Result<String> {
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

#[test]
#[ignore = "hits the live reMarkable cloud; needs RM_CLOUD_DEVICE_TOKEN"]
fn live_reactor_enqueues_digest_job_and_ignores_self_writes() {
    let Ok(token) = std::env::var("RM_CLOUD_DEVICE_TOKEN") else {
        eprintln!("skipping live reactor test: RM_CLOUD_DEVICE_TOKEN unset");
        return;
    };
    if token.is_empty() {
        eprintln!("skipping live reactor test: RM_CLOUD_DEVICE_TOKEN empty");
        return;
    }

    // Dedicated, kept-alive runtime for the raw seed client. Must outlive the whole test.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    // Raw async client (PRODUCTION) used only to SEED the scratch folder.
    let seed = Client::from_device_token(CloudConfig::from_env(), token.clone());

    // ── Scratch folder: rmrs-test/<run-id>/Books ─────────────────────────────────────────
    let run_id = Uuid::new_v4().to_string();
    let watch_path = format!("/{ROOT_TEST_DIR}/{run_id}/Books");
    let (base, run_folder, books, book_id) = rt.block_on(async {
        let base = get_or_create_folder(&seed, ROOT_TEST_DIR, "")
            .await
            .expect("get/create test root folder");
        let run_folder = seed.mkdir(&run_id, &base).await.expect("mk run folder");
        let books = seed.mkdir("Books", &run_folder).await.expect("mk Books");
        let book_id = Uuid::new_v4().to_string();
        seed.put(doc_with_pdf(&book_id, "Book", &books, b"%PDF-v1"))
            .await
            .expect("seed Book");
        (base, run_folder, books, book_id)
    });
    let _ = base;

    // Run the body so we can leave-on-failure: clean up only on Ok.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        live_reactor_body(&rt, &seed, &token, &watch_path, &books, &book_id);
    }));

    // ── Clean up on success; leave on failure ────────────────────────────────────────────
    match result {
        Ok(()) => {
            rt.block_on(async {
                seed.rm(&run_folder).await.expect("cleanup run folder");
            });
        }
        Err(e) => {
            eprintln!(
                "live_reactor test FAILED; leaving {ROOT_TEST_DIR}/{run_id} for inspection"
            );
            drop(rt);
            std::panic::resume_unwind(e);
        }
    }

    // Keep the seed runtime alive until the very end.
    drop(rt);
}

fn live_reactor_body(
    rt: &tokio::runtime::Runtime,
    seed: &Client,
    token: &str,
    watch_path: &str,
    books: &str,
    book_id: &str,
) {
    // ── Hermetic state file (never touch the real daemon state) ──────────────────────────
    let state_file = std::env::temp_dir().join(format!(
        "rmapps-live-reactor-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var("RMAPPS_WATCH_STATE", &state_file);
    // Ensure no stale RM_CLOUD_HOST leaks in from another test in the same process: production.
    std::env::remove_var("RM_CLOUD_HOST");

    // ── App Cloud (PRODUCTION) built from the device token ───────────────────────────────
    let cloud =
        crate::cloud::Cloud::from_device_token(token.to_string()).expect("app Cloud builds");

    let cfg = config_with_watch(watch_path);
    let rules = super::resolved_rules(&cfg);
    let exclude = super::exclude_suffixes(&cfg);
    assert!(
        exclude.iter().any(|s| s == " — Digest"),
        "exclude_suffixes should include the digest suffix, got {exclude:?}"
    );

    let mut state = WatchState::default();
    let mut debouncer = Debouncer::default();
    let far_future = Instant::now() + Duration::from_secs(10_000_000);

    // ── Reconcile #1: baseline ───────────────────────────────────────────────────────────
    super::reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)
        .expect("baseline reconcile");
    let _drained = debouncer.ready(far_future); // clear baseline-induced jobs
    assert!(
        state.baseline.contains_key(book_id),
        "baseline should contain the seeded Book doc id"
    );

    // ── Mutate the Book (content-only bump → new hash + generation) ──────────────────────
    rt.block_on(async {
        seed.put_content_only(book_id, b"%PDF-v2".to_vec())
            .await
            .expect("bump Book content");
    });

    // ── Reconcile #2: detect the change and enqueue exactly one Digest job ───────────────
    super::reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)
        .expect("change reconcile");
    let jobs = debouncer.ready(far_future);

    assert_eq!(
        jobs.len(),
        1,
        "expected exactly one job (Book) after bump; got: {:?}",
        jobs.iter().map(|j| (&j.doc.path, j.action)).collect::<Vec<_>>()
    );
    let job = &jobs[0];
    assert_eq!(job.action, WatchAction::Digest, "job action should be Digest");
    assert_eq!(
        job.doc.path,
        format!("{watch_path}/Book"),
        "job should target the scratch Book path"
    );
    assert_eq!(job.doc.id, book_id, "job should carry the Book doc id");

    // ── Self-write no-loop: a digest-suffixed doc, when changed, must NOT enqueue ────────
    // Seed a "Book — Digest" doc under the same watched folder, baseline it, then bump it.
    let digest_id = Uuid::new_v4().to_string();
    rt.block_on(async {
        seed.put(doc_with_pdf(&digest_id, "Book — Digest", books, b"%PDF-d1"))
            .await
            .expect("seed digest doc");
    });
    // Reconcile to absorb the new digest doc into the baseline (it is excluded from routing,
    // so this enqueues nothing for it; "Book" is unchanged since reconcile #2's baseline).
    super::reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)
        .expect("absorb digest doc reconcile");
    let _drained = debouncer.ready(far_future);

    rt.block_on(async {
        seed.put_content_only(&digest_id, b"%PDF-d2".to_vec())
            .await
            .expect("bump digest doc");
    });
    super::reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)
        .expect("self-write reconcile");
    let jobs = debouncer.ready(far_future);
    assert!(
        jobs.is_empty(),
        "self-write digest doc must never enqueue a job (no-loop); got: {:?}",
        jobs.iter().map(|j| (&j.doc.path, j.action)).collect::<Vec<_>>()
    );

    // Hermeticity + cleanup of the temp state file.
    assert!(state_file.exists(), "reconcile_pass should have written the override state file");
    let _ = std::fs::remove_file(&state_file);
    std::env::remove_var("RMAPPS_WATCH_STATE");
}
