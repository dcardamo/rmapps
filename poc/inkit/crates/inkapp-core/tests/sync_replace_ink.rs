//! Counting-fake transport that distinguishes `push` from `push_replace_ink`.
//! Asserts:
//!  - `publish` only ever calls `push` (zero `push_replace_ink` calls).
//!  - `sync_once` calls `push_replace_ink` for the post-fold push when ink folds.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use inkapp_core::components::notice::Notice;
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_core::crypto::Key;
use inkapp_core::document::{Document, Documents};
use inkapp_core::error::Result;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_core::runtime::{app, App, DocSet};
use inkapp_core::sync::{publish, sync_once, DeviceTransport};

// ── Minimal connector set ────────────────────────────────────────────────────

struct NoCx;
impl ConnectorSet for NoCx {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![]
    }
}

// ── Counting fake transport ──────────────────────────────────────────────────

#[derive(Default)]
struct CountingTransport {
    /// Keys pushed via `push` (ink-preserving).
    pushes: Mutex<Vec<String>>,
    /// Keys pushed via `push_replace_ink` (ink-wiping).
    replace_ink_pushes: Mutex<Vec<String>>,
    /// Queue of canned pull responses; empty → returns empty map.
    canned_pulls: Mutex<VecDeque<HashMap<String, Vec<Vec<Stroke>>>>>,
}

impl CountingTransport {
    fn with_pulls(pulls: Vec<HashMap<String, Vec<Vec<Stroke>>>>) -> Self {
        Self {
            canned_pulls: Mutex::new(pulls.into_iter().collect()),
            ..Self::default()
        }
    }

    fn push_count(&self) -> usize {
        self.pushes.lock().unwrap().len()
    }

    fn replace_ink_count(&self) -> usize {
        self.replace_ink_pushes.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl DeviceTransport for CountingTransport {
    async fn push(&self, key: &str, _pdf: &[u8]) -> Result<()> {
        self.pushes.lock().unwrap().push(key.to_string());
        Ok(())
    }

    async fn push_replace_ink(&self, key: &str, _pdf: &[u8]) -> Result<()> {
        self.replace_ink_pushes.lock().unwrap().push(key.to_string());
        Ok(())
    }

    async fn delete(&self, _key: &str) {}

    async fn pull(&self, _p: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
        self.canned_pulls
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default()
    }
}

// ── App helpers ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum TestMsg {
    #[allow(dead_code)]
    Noop,
}

fn view(_m: &(), _cx: &NoCx) -> Documents<TestMsg> {
    Documents(vec![Document::keyed(
        "doc-a",
        inkapp_core::flow![Notice::line("hello")],
    )])
}
fn update(_msg: TestMsg, _m: &mut (), _cx: &NoCx) {}

fn build_app() -> App<(), TestMsg, NoCx> {
    app(())
        .connector(NoCx)
        .update(update as fn(TestMsg, &mut (), &NoCx))
        .view(view as fn(&(), &NoCx) -> Documents<TestMsg>)
        .key(Key::from_bytes([11u8; 32]))
        .build()
}

// ── App with ink that decodes to a message and mutates the model ─────────────
// The fold changes the model's counter, causing the rendered PDF hash to differ
// from the pre-fold hash → reconcile emits an Update op → cycle.rendered fires.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::ink::RegionInk;

#[derive(Clone, Debug, PartialEq)]
enum InkMsg {
    Tapped,
}

struct TapTile;

impl Component for TapTile {
    type Msg = InkMsg;

    fn render(&self, _cx: &mut RenderCx) -> String {
        // Big region so the test stroke hits it.
        "#region(\"tap\", [#block(height: 300pt, fill: luma(240))[tap]], breakable: true)\n\n"
            .to_string()
    }

    fn decode(&self, ink: &[RegionInk], _manifest: &inkapp_core::manifest::Manifest) -> Vec<InkMsg> {
        if ink.iter().any(|r| r.region == "tap" && !r.strokes.is_empty()) {
            vec![InkMsg::Tapped]
        } else {
            vec![]
        }
    }
}

/// Model holds a counter; update increments it so each fold changes the render.
#[derive(Default, Clone)]
struct CountModel {
    taps: u32,
}

fn tap_view(m: &CountModel, _cx: &NoCx) -> Documents<InkMsg> {
    // TapTile first so it's at the top of the page (y≈540 stroke hits it).
    // The counter Notice follows — it changes the Typst source on each fold
    // so the re-render produces a new content hash, triggering an Update op.
    Documents(vec![Document::keyed(
        "doc-a",
        inkapp_core::flow![
            TapTile,
            Notice::line(&format!("taps: {}", m.taps))
        ],
    )])
}

fn tap_update(msg: InkMsg, m: &mut CountModel, _cx: &NoCx) {
    match msg {
        InkMsg::Tapped => m.taps += 1,
    }
}

fn build_tap_app() -> App<CountModel, InkMsg, NoCx> {
    app(CountModel::default())
        .connector(NoCx)
        .update(tap_update as fn(InkMsg, &mut CountModel, &NoCx))
        .view(tap_view as fn(&CountModel, &NoCx) -> Documents<InkMsg>)
        .key(Key::from_bytes([13u8; 32]))
        .build()
}

fn stroke_at(x: f64, y: f64) -> Stroke {
    Stroke {
        points: vec![PdfPoint { x, y }],
        highlighter: false,
    }
}

fn ink_for(key: &str, stroke: Stroke) -> HashMap<String, Vec<Vec<Stroke>>> {
    let mut m = HashMap::new();
    m.insert(key.to_string(), vec![vec![stroke]]);
    m
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// `publish` must call only `push`, never `push_replace_ink`.
#[tokio::test]
async fn publish_uses_push_not_replace_ink() {
    let mut application = build_app();
    let mut set = DocSet::default();
    let t = CountingTransport::default();

    publish(&mut application, &mut set, &t).await.unwrap();

    assert!(t.push_count() >= 1, "publish must call push at least once");
    assert_eq!(
        t.replace_ink_count(),
        0,
        "publish must never call push_replace_ink"
    );
}

/// `sync_once` with folded ink must call `push_replace_ink` for the post-fold
/// push, never `push` (the post-fold path).
///
/// Setup: publish first so manifests are in `set`; then sync with canned ink
/// that lands in the tap region. The model counter increments, so the re-render
/// produces a different hash → reconcile emits an Update → cycle.rendered fires.
#[tokio::test]
async fn sync_once_post_fold_uses_replace_ink() {
    let mut application = build_tap_app();
    let mut set = DocSet::default();

    // Publish so manifests are in `set` for ink attribution.
    let t0 = CountingTransport::default();
    publish(&mut application, &mut set, &t0).await.unwrap();

    // Queue ink that lands in the tap region of doc-a. PDF space: y=0 is bottom,
    // y=560 is top. The TapTile 300pt block fills from the top: ~y=[244,544].
    // y=540 is well inside that range (same coordinate as existing archive test).
    let scripted = ink_for("doc-a", stroke_at(50.0, 540.0));
    let t = CountingTransport::with_pulls(vec![scripted]);

    let cycle = sync_once(&mut application, &mut set, &t).await.unwrap();

    // The fold must have decoded the ink and re-rendered.
    assert!(
        !cycle.decoded.is_empty(),
        "ink must decode to a message; got {:?}",
        cycle.decoded
    );
    assert!(
        !cycle.rendered.is_empty(),
        "fold must produce at least one rendered doc (model changed → hash changed → Update op)"
    );

    // The re-rendered doc must have gone through push_replace_ink, not push.
    assert!(
        t.replace_ink_count() >= 1,
        "post-fold push must call push_replace_ink"
    );
    assert_eq!(
        t.push_count(),
        0,
        "sync_once post-fold must not call plain push"
    );
}

/// Verify the default trait impl: a transport that only implements `push`
/// (no override) gets `push_replace_ink` routed through `push`.
#[tokio::test]
async fn default_push_replace_ink_delegates_to_push() {
    // A transport that uses the DEFAULT push_replace_ink (no override).
    // Tracks push calls via a shared counter accessible after trait-object use.
    let counter = Arc::new(Mutex::new(0usize));

    struct DefaultTransport {
        pushes: Arc<Mutex<usize>>,
    }
    #[async_trait::async_trait]
    impl DeviceTransport for DefaultTransport {
        async fn push(&self, _key: &str, _pdf: &[u8]) -> Result<()> {
            *self.pushes.lock().unwrap() += 1;
            Ok(())
        }
        async fn delete(&self, _key: &str) {}
        async fn pull(&self, _p: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
            HashMap::new()
        }
        // push_replace_ink NOT overridden — default delegates to push.
    }

    let t = DefaultTransport {
        pushes: counter.clone(),
    };
    // Call through the trait to exercise the default impl.
    t.push_replace_ink("key", b"pdf").await.unwrap();
    assert_eq!(
        *counter.lock().unwrap(),
        1,
        "default push_replace_ink must delegate to push"
    );
}
