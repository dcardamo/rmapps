//! Orchestration: run the full pipeline for every watched path.
//!
//! [`run`] drives the pipeline end-to-end:
//!   list → fetch → ingest → extract → build → deploy → persist state.
//!
//! The skip predicate fires when page hashes haven't changed since the last
//! successful run, avoiding redundant uploads.

use std::path::Path;

use crate::config::Config;
use crate::deploy::{Backend, CloudDoc};
use crate::digest_doc::DigestMeta;
use crate::extract::{extract, Mark};
use crate::ingest::ingest;
use crate::render::compile;
use crate::state::State;

/// Options for a single `run` invocation.
pub struct Opts {
    /// Skip uploading (still fetches + generates).
    pub dry_run: bool,
    /// If set, write outputs here instead of uploading (for `--local`).
    pub local_output: Option<std::path::PathBuf>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the full pipeline for every watched path in `cfg`, using `backend` for
/// cloud list/fetch/put operations.
///
/// State is persisted to `state_path` after each successful per-doc run so that
/// a crash in the middle re-processes only the failed doc on the next run.
pub fn run(
    cfg: &Config,
    backend: &dyn Backend,
    state_path: &Path,
    opts: &Opts,
) -> anyhow::Result<()> {
    let mut state = State::load(state_path)?;

    let exclude = vec![
        cfg.output.annotated_suffix.clone(),
        cfg.output.digest_suffix.clone(),
    ];

    for root in &cfg.watched_paths {
        let docs = backend.list(root, &exclude)?;
        for doc in docs {
            process_doc(cfg, backend, &doc, &mut state, state_path, opts)?;
        }
    }

    Ok(())
}

/// Process a single `CloudDoc` through the full pipeline.
fn process_doc(
    cfg: &Config,
    backend: &dyn Backend,
    doc: &CloudDoc,
    state: &mut State,
    state_path: &Path,
    opts: &Opts,
) -> anyhow::Result<()> {
    let prev = state.docs.entry(doc.path.clone()).or_default();

    let bundle_path = match backend.fetch(doc)? {
        Some(p) => p,
        None => {
            eprintln!("rmdigest: fetch returned None for {}, skipping", doc.path);
            return Ok(());
        }
    };

    let ing = ingest(&bundle_path, prev)?;

    // Skip if nothing changed since a prior successful run.
    if ing.changed.is_empty() && !prev.page_hashes.is_empty() {
        eprintln!("rmdigest: {} unchanged, skipping", doc.path);
        return Ok(());
    }

    // Dirty (or first sight): regenerate from ALL pages.
    let device = crate::device::get_device(&cfg.device)?;
    let all_pages: Vec<usize> = (0..ing.bundle.pages().len()).collect();
    let marks = extract(&ing.bundle, &all_pages)?;
    let meta = digest_meta(&ing.bundle, &marks);

    // Single pure-typst output: the hyperlinked digest followed by a rasterized
    // image of each annotated page (highlights painted on). No lopdf page-tree
    // surgery, so it works on any source PDF and carries real #link jumps.
    let (src, assets) = crate::linked_doc::build_linked(&meta, &marks, &ing.bundle, &device)?;
    let digest_pdf = compile(&src, &assets)?;

    if opts.dry_run {
        // Generate but neither upload nor persist state: a dry run must not poison
        // the hash cache, or the next real run would skip and never upload.
        eprintln!("rmdigest: [dry-run] generated digest for {}", doc.path);
        return Ok(());
    }

    let stage = tempfile::tempdir()?;

    // Stage and put the digest PDF.
    let digest_name = format!("{}{}", meta.title, cfg.output.digest_suffix);
    let digest_file = stage.path().join(format!("{}.pdf", digest_name));
    std::fs::write(&digest_file, &digest_pdf)?;
    backend.put(&digest_file, &doc.folder, &digest_name)?;

    // Persist state only after the upload succeeds, so a crash re-processes.
    prev.cloud_version = doc.version.clone();
    prev.page_hashes = ing.new_hashes;
    state.save(state_path)?;

    eprintln!("rmdigest: processed {}", doc.path);
    Ok(())
}

// ---------------------------------------------------------------------------
// digest_meta: build DigestMeta from a bundle + marks
// ---------------------------------------------------------------------------

/// Build [`DigestMeta`] from a bundle and extracted marks.
///
/// Title comes from the bundle's visible name (fallback: "Untitled").
/// Counts are derived from the marks slice.
/// Author and date_range are left empty (no reliable source in rmapi bundles).
pub fn digest_meta(bundle: &rmfiles::Bundle, marks: &[Mark]) -> DigestMeta {
    let title = {
        let name = bundle.metadata().visible_name.clone();
        if name.is_empty() {
            "Untitled".to_string()
        } else {
            name
        }
    };

    let n_highlights = marks
        .iter()
        .filter(|m| matches!(m, Mark::Highlight { .. }))
        .count();

    let n_notes = marks
        .iter()
        .filter(|m| matches!(m, Mark::Note { .. } | Mark::InsertedPage { .. }))
        .count();

    DigestMeta {
        title,
        author: String::new(),
        n_highlights,
        n_notes,
        date_range: String::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Deploy, Output};
    use crate::deploy::CloudDoc;
    use crate::state::State;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    // ── Fake backend ─────────────────────────────────────────────────────────

    type PutLog = Arc<Mutex<Vec<(String, String, Vec<u8>)>>>;

    struct FakeBackend {
        /// The fixture doc to return from list().
        fixture: CloudDoc,
        /// The local path that fetch() returns.
        fixture_path: PathBuf,
        /// Recorded put() calls: (folder, name, bytes).
        puts: PutLog,
    }

    impl Backend for FakeBackend {
        fn list(&self, _root: &str, _exclude: &[String]) -> anyhow::Result<Vec<CloudDoc>> {
            Ok(vec![self.fixture.clone()])
        }

        fn fetch(&self, _doc: &CloudDoc) -> anyhow::Result<Option<PathBuf>> {
            Ok(Some(self.fixture_path.clone()))
        }

        fn put(&self, pdf: &Path, folder: &str, name: &str) -> anyhow::Result<()> {
            let bytes = std::fs::read(pdf)?;
            self.puts
                .lock()
                .unwrap()
                .push((folder.to_string(), name.to_string(), bytes));
            Ok(())
        }
    }

    fn fixture_path() -> PathBuf {
        let manifest = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest).join("tests/fixtures/stamped-labels.rmdoc")
    }

    fn fake_cfg() -> Config {
        Config {
            device: "paper-pro".into(),
            watched_paths: vec!["/Books".into()],
            deploy: Deploy::default(),
            output: Output::default(),
        }
    }

    // ── Unit: digest_meta counts ──────────────────────────────────────────────

    #[test]
    fn digest_meta_counts_marks() {
        let bundle_path = fixture_path();
        let bundle = rmfiles::Bundle::open(&bundle_path).expect("open fixture");
        let pages = bundle.pages();
        let all_indices: Vec<usize> = pages.iter().map(|p| p.index).collect();
        let marks = extract(&bundle, &all_indices).expect("extract");

        let meta = digest_meta(&bundle, &marks);

        // Title must be non-empty (the fixture has a visible name).
        assert!(!meta.title.is_empty(), "title must not be empty");

        // Counts must agree with manual tally from the marks vec.
        let exp_hl = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .count();
        let exp_n = marks
            .iter()
            .filter(|m| matches!(m, Mark::Note { .. } | Mark::InsertedPage { .. }))
            .count();
        assert_eq!(meta.n_highlights, exp_hl);
        assert_eq!(meta.n_notes, exp_n);
    }

    // ── Unit: skip predicate ──────────────────────────────────────────────────

    #[test]
    fn skip_when_unchanged() {
        // Build a DocState with non-empty page_hashes that exactly match what
        // ingest() would compute for the fixture. Run ingest twice: first run
        // produces new_hashes; second run with those hashes should yield
        // changed.is_empty() == true.
        let bundle_path = fixture_path();
        let prev1 = crate::state::DocState::default();
        let run1 = ingest(&bundle_path, &prev1).expect("ingest run1");

        let prev2 = crate::state::DocState {
            cloud_version: None,
            page_hashes: run1.new_hashes.clone(),
            digest_uuids: vec![],
        };
        let run2 = ingest(&bundle_path, &prev2).expect("ingest run2");

        assert!(
            run2.changed.is_empty(),
            "second ingest should report no changes"
        );
        // Verify the skip condition matches.
        assert!(
            !prev2.page_hashes.is_empty(),
            "prev hashes must be non-empty for the skip to fire"
        );
    }

    // ── Integration: two puts on first run, zero on second ───────────────────

    #[test]
    fn integration_two_puts_then_skip() {
        let fixture = fixture_path();
        let puts = Arc::new(Mutex::new(Vec::new()));

        let doc = CloudDoc {
            path: "/Books/StampedLabels".to_string(),
            name: "stamped-labels".to_string(),
            folder: "/Books".to_string(),
            version: None,
        };

        let backend = FakeBackend {
            fixture: doc,
            fixture_path: fixture,
            puts: puts.clone(),
        };

        let cfg = fake_cfg();
        let state_dir = tempfile::tempdir().expect("tempdir");
        let state_path = state_dir.path().join("state.json");
        let opts = Opts {
            dry_run: false,
            local_output: None,
        };

        // --- First run ---
        run(&cfg, &backend, &state_path, &opts).expect("first run");

        let first_puts = puts.lock().unwrap().clone();
        assert_eq!(
            first_puts.len(),
            1,
            "expected 1 put (the single linked digest) on first run, got {}",
            first_puts.len()
        );

        // The single output ends with the digest suffix.
        let has_digest = first_puts
            .iter()
            .any(|(_, name, _)| name.ends_with(&cfg.output.digest_suffix));
        assert!(has_digest, "expected a put ending with digest suffix");

        // It must be a valid PDF.
        for (_, name, bytes) in &first_puts {
            lopdf::Document::load_mem(bytes)
                .unwrap_or_else(|e| panic!("put '{}' is not a valid PDF: {e}", name));
        }

        // state.json must exist with non-empty page_hashes.
        assert!(state_path.exists(), "state.json must be written");
        let state = State::load(&state_path).expect("load state");
        let doc_state = state
            .docs
            .get("/Books/StampedLabels")
            .expect("doc state must exist");
        assert!(!doc_state.page_hashes.is_empty(), "page_hashes must be set");

        // --- Second run (unchanged fixture → skip) ---
        puts.lock().unwrap().clear();
        run(&cfg, &backend, &state_path, &opts).expect("second run");

        let second_puts = puts.lock().unwrap().clone();
        assert_eq!(
            second_puts.len(),
            0,
            "expected 0 puts on unchanged second run, got {}",
            second_puts.len()
        );
    }

    #[test]
    fn dry_run_does_not_poison_state() {
        let fixture = fixture_path();
        let puts = Arc::new(Mutex::new(Vec::new()));
        let doc = CloudDoc {
            path: "/Books/StampedLabels".to_string(),
            name: "stamped-labels".to_string(),
            folder: "/Books".to_string(),
            version: None,
        };
        let backend = FakeBackend {
            fixture: doc,
            fixture_path: fixture,
            puts: puts.clone(),
        };
        let cfg = fake_cfg();
        let state_dir = tempfile::tempdir().expect("tempdir");
        let state_path = state_dir.path().join("state.json");

        // Dry run: generates but uploads nothing and must NOT persist hashes.
        run(
            &cfg,
            &backend,
            &state_path,
            &Opts {
                dry_run: true,
                local_output: None,
            },
        )
        .expect("dry run");
        assert_eq!(puts.lock().unwrap().len(), 0, "dry run must not upload");
        if let Ok(state) = State::load(&state_path) {
            assert!(
                state
                    .docs
                    .get("/Books/StampedLabels")
                    .map(|d| d.page_hashes.is_empty())
                    .unwrap_or(true),
                "dry run must not persist page hashes"
            );
        }

        // A subsequent REAL run must still upload (proving state wasn't poisoned).
        run(
            &cfg,
            &backend,
            &state_path,
            &Opts {
                dry_run: false,
                local_output: None,
            },
        )
        .expect("real run");
        assert_eq!(
            puts.lock().unwrap().len(),
            1,
            "real run after a dry run must upload the digest"
        );
    }
}
