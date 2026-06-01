//! Reactor integration test against the in-repo fake cloud.
//!
//! Proves the reactor pipeline end-to-end:
//!   snapshot → diff → resolve-paths → self-write filter → route → debounce-enqueue.
//! We assert the JOBS the reactor enqueues — we do NOT run the real digest/readback
//! actions (those are covered by Task 5 and the Task 10 live e2e).
//!
//! Runtime/seam constraints (see Task 9 brief):
//!   * The app `Cloud` is SYNCHRONOUS and owns its own tokio runtime (it calls
//!     `block_on` internally), so this test MUST be a plain `#[test]`, never
//!     `#[tokio::test]` — a nested runtime would panic.
//!   * The fake cloud is an async axum server. We spawn it on a SEPARATE,
//!     kept-alive multi-thread runtime and keep that runtime alive for the whole
//!     test so the server task keeps running.
//!   * The app `Cloud` reads its host from `RM_CLOUD_HOST` via `Config::from_env()`,
//!     so we set that env var BEFORE constructing the app Cloud.
//!
//! Both scenarios (happy path + self-write no-loop) run inside a SINGLE `#[test]`
//! against ONE fake instance using two distinct folders. This is deliberate: the
//! `RM_CLOUD_HOST` env var is process-global, so two separate tests racing to set
//! it would be flaky. One test, one fake, one host setting — fully deterministic.

use std::time::{Duration, Instant};

use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config as CloudConfig, DocFiles, Metadata};

use crate::config::{Config, WatchAction};
use crate::watch::debounce::Debouncer;
use crate::watch::state::WatchState;

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

/// A minimal rmapps Config with a `[digest]` section (so `exclude_suffixes` yields the
/// digest suffixes) and one watch rule per supplied path. Deserialized from TOML so the
/// rmdigest defaults (digest_suffix " — Digest", annotated_suffix " — Annotated") apply.
fn config_with_watch(rules: &[(&str, &str)]) -> Config {
    let mut toml = String::from(
        r#"
[digest]
device = "test-device"
watched_paths = ["/Books"]
"#,
    );
    for (path, debounce) in rules {
        toml.push_str(&format!(
            "\n[[watch]]\npath = \"{path}\"\naction = \"digest\"\ndebounce = \"{debounce}\"\n"
        ));
    }
    let cfg: Config = toml::from_str(&toml).expect("config TOML parses");
    cfg.validate().expect("config validates");
    cfg
}

#[test]
fn reactor_enqueues_digest_job_and_ignores_self_writes() {
    // ── Fake cloud on a dedicated, kept-alive runtime ────────────────────────────────
    // `rt` MUST outlive the whole test or the axum server task stops.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let fake = rt.block_on(FakeCloud::spawn());

    // Raw async client used only to SEED the fake (mkdir / put / put_content_only).
    let seed = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");

    // Seed: /Books with a user doc "Book", and /Reading with a digest-suffixed doc that
    // must be treated as a self-write. Capture the doc ids so we can mutate them later.
    let book_id = "11111111-1111-4111-8111-111111111111".to_string();
    let digest_id = "22222222-2222-4222-8222-222222222222".to_string();
    rt.block_on(async {
        let books = seed.mkdir("Books", "").await.unwrap();
        seed.put(doc_with_pdf(&book_id, "Book", &books, b"%PDF-v1"))
            .await
            .unwrap();

        let reading = seed.mkdir("Reading", "").await.unwrap();
        seed.put(doc_with_pdf(
            &digest_id,
            "Book — Digest", // ends with the default digest suffix
            &reading,
            b"%PDF-d1",
        ))
        .await
        .unwrap();
    });

    // ── App Cloud pointed at the fake via RM_CLOUD_HOST ──────────────────────────────
    // Set the host BEFORE constructing the app Cloud (Config::from_env reads it once).
    std::env::set_var("RM_CLOUD_HOST", &fake.base);
    let cloud = crate::cloud::Cloud::from_device_token("test-token".into())
        .expect("app Cloud builds against fake");

    // Watch both folders for Digest so a job WOULD be routed for either if it survived
    // the self-write filter. The digest-suffixed doc must still be dropped.
    let cfg = config_with_watch(&[("/Books", "30s"), ("/Reading", "30s")]);
    let rules = super::resolved_rules(&cfg);
    let exclude = super::exclude_suffixes(&cfg);
    // Sanity: the digest section yielded the two self-write suffixes.
    assert!(
        exclude.iter().any(|s| s == " — Digest"),
        "exclude_suffixes should include the digest suffix, got {exclude:?}"
    );

    let mut state = WatchState::default();
    let mut debouncer = Debouncer::default();

    // Reconcile #1: establish the baseline (snapshot of the seeded docs). Any jobs
    // enqueued here (the digest-suffixed doc is already excluded; "Book" enqueues
    // because the baseline starts empty) are drained/cleared so the next pass starts
    // from a clean debouncer keyed only by REAL changes.
    super::reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)
        .expect("baseline reconcile");
    let far_future = Instant::now() + Duration::from_secs(10_000_000);
    let _drained = debouncer.ready(far_future); // clear baseline-induced jobs

    // Baseline must have observed BOTH docs (proves the fake actually served the app
    // Cloud's snapshot — not a silent short-circuit).
    assert!(
        state.baseline.contains_key(&book_id),
        "baseline should contain the seeded Book doc id"
    );
    assert!(
        state.baseline.contains_key(&digest_id),
        "baseline should contain the seeded digest doc id (snapshot sees all docs)"
    );

    // ── Mutate both docs (bumps hash + generation) ───────────────────────────────────
    rt.block_on(async {
        seed.put_content_only(&book_id, b"%PDF-v2".to_vec())
            .await
            .unwrap();
        seed.put_content_only(&digest_id, b"%PDF-d2".to_vec())
            .await
            .unwrap();
    });

    // Reconcile #2: detect the changes and enqueue jobs.
    super::reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)
        .expect("change reconcile");

    let jobs = debouncer.ready(far_future);

    // ── Assertions ───────────────────────────────────────────────────────────────────
    // Test 1 (happy path): exactly one job, for "Book", Digest, at /Books/Book.
    // Test 2 (self-write no loop): the digest-suffixed doc produced NO job — it is
    // dropped by list_recursive's exclude + filter_self_writes, so it never routes.
    assert_eq!(
        jobs.len(),
        1,
        "expected exactly one job (Book), self-write digest doc must be filtered; got: {:?}",
        jobs.iter().map(|j| (&j.doc.path, j.action)).collect::<Vec<_>>()
    );
    let job = &jobs[0];
    assert_eq!(job.action, WatchAction::Digest, "job action should be Digest");
    assert_eq!(job.doc.path, "/Books/Book", "job should target /Books/Book");
    assert_eq!(job.doc.id, book_id, "job should carry the Book doc id");

    // Explicit no-loop check: no job references the digest doc id or its path.
    assert!(
        !jobs.iter().any(|j| j.doc.id == digest_id),
        "self-write digest doc must never be enqueued (no-loop guarantee)"
    );

    // Keep the fake's runtime alive until the very end of the test (the axum server
    // task runs on it; dropping earlier would stop serving the app Cloud).
    drop(rt);
}
