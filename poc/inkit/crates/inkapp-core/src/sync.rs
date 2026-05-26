//! Device-agnostic on-device deployment: the `DeviceTransport` seam plus the
//! generic publish/sync engine that drives an `App` over any transport. Only
//! keys, PDF bytes, and PDF-space strokes cross this boundary — no reMarkable (or
//! any device) specifics live here.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use crate::connector::ConnectorSet;
use crate::error::Result;
use crate::ink::Stroke;
use crate::reconcile::DocOp;
use crate::runtime::{App, Cycle, DocSet};

/// A device's sync transport: how rendered documents reach the hardware and how
/// the user's ink comes back. Implemented once per device family (reMarkable
/// today, in `rm-device`). Async (the reMarkable backend talks to the cloud over
/// the network) and object-safe so the facade can dispatch on config.
#[async_trait::async_trait]
pub trait DeviceTransport: Send + Sync {
    /// Push a rendered document (its key + PDF bytes) to the device.
    async fn push(&self, key: &str, pdf: &[u8]) -> Result<()>;
    /// Delete a document by key. Best-effort: a missing document is not an error.
    async fn delete(&self, key: &str);
    /// Pull all device ink, keyed by document key, as PDF-space strokes.
    /// `page_h_by_key` lets the backend decode each document at its page height.
    async fn pull(&self, page_h_by_key: &HashMap<String, f64>)
        -> HashMap<String, Vec<Vec<Stroke>>>;
}

/// Render the app's full document set and push every document to the device.
pub async fn publish<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    set: &mut DocSet,
    transport: &dyn DeviceTransport,
) -> Result<()> {
    let rendered = app.render(set).await?;
    for rd in &rendered {
        transport.push(&rd.key.0, &rd.pdf).await?;
    }
    println!("published {} document(s)", rendered.len());
    Ok(())
}

/// Render to rebuild the set, pull device ink, fold one cycle, then apply the
/// resulting ops to the device (delete removed, push created/updated).
pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    set: &mut DocSet,
    transport: &dyn DeviceTransport,
) -> Result<Cycle<Msg>> {
    // Rebuild the in-memory set from current state so pulled ink attributes
    // against the same manifests/page heights that were last published.
    app.render(set).await?;
    let page_h: HashMap<String, f64> = set
        .keys()
        .into_iter()
        .filter_map(|k| set.page_h(&k).map(|h| (k.0, h)))
        .collect();
    let ink = transport.pull(&page_h).await;
    let cycle = app.step(set, &ink).await?;
    for op in &cycle.ops {
        if let DocOp::Delete(k) = op {
            transport.delete(&k.0).await;
        }
    }
    for rd in &cycle.rendered {
        transport.push(&rd.key.0, &rd.pdf).await?;
    }
    println!(
        "synced: {} message(s), {} op(s)",
        cycle.decoded.len(),
        cycle.ops.len()
    );
    Ok(cycle)
}

/// Drive the device round-trip: publish the current document set, then loop —
/// every `interval`, run one `sync_once` cycle and log decoded messages and
/// reconcile ops. Returns when `shutdown` resolves.
///
/// Transport-agnostic by construction. The shutdown future is a parameter so
/// callers can plumb `tokio::signal::ctrl_c()`, a oneshot, a `Notify`, or an
/// immediately-ready future (for tests).
pub async fn serve<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    set: &mut DocSet,
    transport: &dyn DeviceTransport,
    interval: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<()>
where
    Msg: Clone + std::fmt::Debug,
{
    publish(app, set, transport).await?;
    let mut shutdown = Box::pin(shutdown);
    let mut n: u64 = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = &mut shutdown => {
                println!("serve: shutdown");
                return Ok(());
            }
        }
        n += 1;
        let cycle = sync_once(app, set, transport).await?;
        let pushes = cycle
            .ops
            .iter()
            .filter(|o| matches!(o, DocOp::Create(_) | DocOp::Update(_)))
            .count();
        let deletes = cycle
            .ops
            .iter()
            .filter(|o| matches!(o, DocOp::Delete(_)))
            .count();
        println!(
            "cycle {n}: decoded={} ops=push:{} delete:{}",
            cycle.decoded.len(),
            pushes,
            deletes
        );
        for m in &cycle.decoded {
            println!("  msg: {m:?}");
        }
        for op in &cycle.ops {
            println!("  op:  {op:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::notice::Notice;
    use crate::connector::Connector;
    use crate::crypto::Key;
    use crate::document::{Document, Documents};
    use crate::runtime::{app, App};
    use std::sync::{Arc, Mutex};

    struct NoCx;
    impl ConnectorSet for NoCx {
        fn connectors(&self) -> Vec<Arc<dyn Connector>> {
            vec![]
        }
    }

    #[derive(Clone)]
    enum TestMsg {
        #[allow(dead_code)]
        Noop,
    }

    fn view(_m: &(), _cx: &NoCx) -> Documents<TestMsg> {
        Documents(vec![Document::keyed(
            "doc-a",
            crate::flow![Notice::line("hello")],
        )])
    }
    fn update(_msg: TestMsg, _m: &mut (), _cx: &NoCx) {}

    fn build_test_app() -> App<(), TestMsg, NoCx> {
        app(())
            .connector(NoCx)
            .update(update as fn(TestMsg, &mut (), &NoCx))
            .view(view as fn(&(), &NoCx) -> Documents<TestMsg>)
            .key(Key::from_bytes([7u8; 32]))
            .build()
    }

    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeTransport {
        pushed: Mutex<Vec<(String, usize)>>,
        canned_pulls: Mutex<VecDeque<HashMap<String, Vec<Vec<Stroke>>>>>,
        pulls_done: Mutex<usize>,
        deleted: Mutex<Vec<String>>,
    }

    impl FakeTransport {
        /// Seed the queue of pull responses (front = first call). Empty queue → next
        /// `pull` returns an empty HashMap.
        #[allow(dead_code)]
        fn with_pulls(pulls: Vec<HashMap<String, Vec<Vec<Stroke>>>>) -> Self {
            Self {
                canned_pulls: Mutex::new(pulls.into_iter().collect()),
                ..Self::default()
            }
        }
    }

    #[async_trait::async_trait]
    impl DeviceTransport for FakeTransport {
        async fn push(&self, key: &str, pdf: &[u8]) -> Result<()> {
            self.pushed
                .lock()
                .unwrap()
                .push((key.to_string(), pdf.len()));
            Ok(())
        }
        async fn delete(&self, key: &str) {
            self.deleted.lock().unwrap().push(key.to_string());
        }
        async fn pull(&self, _p: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
            *self.pulls_done.lock().unwrap() += 1;
            self.canned_pulls
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default()
        }
    }

    use crate::component::{Component, RenderCx};
    use crate::geometry::PdfPoint;
    use crate::ink::{RegionInk, Stroke};

    #[derive(Clone, Debug, PartialEq)]
    enum ArchiveMsg {
        Archive(String),
    }

    struct ArchiveTile {
        key: String,
    }

    impl Component for ArchiveTile {
        type Msg = ArchiveMsg;

        fn render(&self, _cx: &mut RenderCx) -> String {
            let region = format!("archive-{}", self.key);
            // A tall block at the top of the page that the test stroke definitely
            // lands inside. Region label is unique per doc.
            // The region body is itself a sized block so measure() inside the
            // region macro reports the full extent — otherwise it sizes to the
            // inner text glyphs and the test stroke misses it.
            // breakable:true uses layout() to capture the column width and emits
            // flow-start/flow-end markers — full-column rect (vs. content-glyph rect).
            format!(
                "#region(\"{region}\", [#block(height: 300pt, fill: luma(240))[archive]], breakable: true)\n\n"
            )
        }

        fn decode(
            &self,
            ink: &[RegionInk],
            _manifest: &crate::manifest::Manifest,
        ) -> Vec<ArchiveMsg> {
            let want = format!("archive-{}", self.key);
            if ink
                .iter()
                .any(|r| r.region == want && !r.strokes.is_empty())
            {
                vec![ArchiveMsg::Archive(self.key.clone())]
            } else {
                vec![]
            }
        }
    }

    #[derive(Default, Clone)]
    struct ArchiveModel(Vec<String>);

    fn archive_view(m: &ArchiveModel, _cx: &NoCx) -> Documents<ArchiveMsg> {
        Documents(
            m.0.iter()
                .map(|k| {
                    let key = k.clone();
                    Document::keyed(&key, crate::flow![ArchiveTile { key: key.clone() }])
                })
                .collect(),
        )
    }

    fn archive_update(msg: ArchiveMsg, m: &mut ArchiveModel, _cx: &NoCx) {
        match msg {
            ArchiveMsg::Archive(k) => m.0.retain(|x| x != &k),
        }
    }

    fn build_archive_app() -> App<ArchiveModel, ArchiveMsg, NoCx> {
        app(ArchiveModel(vec!["doc-a".into()]))
            .connector(NoCx)
            .update(archive_update as fn(ArchiveMsg, &mut ArchiveModel, &NoCx))
            .view(archive_view as fn(&ArchiveModel, &NoCx) -> Documents<ArchiveMsg>)
            .key(Key::from_bytes([9u8; 32]))
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

    #[tokio::test]
    async fn publish_pushes_every_rendered_doc() {
        let mut application = build_test_app();
        let mut set = DocSet::default();
        let t = FakeTransport::default();
        publish(&mut application, &mut set, &t).await.unwrap();
        let pushed = t.pushed.lock().unwrap();
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].0, "doc-a");
        assert!(pushed[0].1 > 0, "pushed a non-empty pdf");
    }

    #[tokio::test]
    async fn sync_once_consults_transport_and_no_ops_without_ink() {
        let mut application = build_test_app();
        let mut set = DocSet::default();
        let t = FakeTransport::default();
        let cycle = sync_once(&mut application, &mut set, &t).await.unwrap();
        assert_eq!(*t.pulls_done.lock().unwrap(), 1);
        assert!(cycle.ops.is_empty(), "no device ops without ink");
        assert!(cycle.decoded.is_empty());
    }

    #[tokio::test]
    async fn serve_publishes_before_first_pull() {
        let mut application = build_archive_app();
        let mut set = DocSet::default();
        let t = FakeTransport::default();
        // Shutdown immediately ready — loop body must not run, but the initial
        // publish must complete.
        serve(
            &mut application,
            &mut set,
            &t,
            Duration::from_millis(1),
            std::future::ready(()),
        )
        .await
        .unwrap();
        assert!(
            !t.pushed.lock().unwrap().is_empty(),
            "initial publish must push before any pull"
        );
        assert_eq!(
            *t.pulls_done.lock().unwrap(),
            0,
            "no pull before the first sleep elapses"
        );
    }

    #[tokio::test]
    async fn sync_once_archives_doc_on_ink() {
        let mut application = build_archive_app();
        let mut set = DocSet::default();
        // Publish once so the manifest is in `set` for ink attribution.
        let t0 = FakeTransport::default();
        publish(&mut application, &mut set, &t0).await.unwrap();
        // Now queue ink that lands inside the archive region of doc-a.
        let scripted = ink_for("doc-a", stroke_at(50.0, 540.0));
        let t = FakeTransport::with_pulls(vec![scripted]);
        let cycle = sync_once(&mut application, &mut set, &t).await.unwrap();
        assert_eq!(
            cycle.decoded,
            vec![ArchiveMsg::Archive("doc-a".into())],
            "ink in archive region decodes to Archive(doc-a)"
        );
        assert!(
            cycle
                .ops
                .iter()
                .any(|o| matches!(o, DocOp::Delete(k) if k.0 == "doc-a")),
            "removing doc-a from model yields a Delete op; got {:?}",
            cycle.ops
        );
        assert_eq!(
            t.deleted.lock().unwrap().as_slice(),
            &["doc-a".to_string()],
            "transport saw the delete"
        );
    }

    #[tokio::test]
    async fn serve_two_cycles_decode_then_quiet() {
        use std::sync::Arc;
        use tokio::sync::Notify;
        let mut application = build_archive_app();
        let mut set = DocSet::default();
        let scripted = ink_for("doc-a", stroke_at(50.0, 540.0));
        let t = FakeTransport::with_pulls(vec![scripted, HashMap::new()]);
        let notify = Arc::new(Notify::new());
        let shutdown_notify = notify.clone();
        let shutdown = async move { shutdown_notify.notified().await };
        let poller = {
            let notify = notify.clone();
            let t = &t;
            async move {
                loop {
                    if *t.pulls_done.lock().unwrap() >= 2 {
                        notify.notify_one();
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        };
        let (res, _) = tokio::join!(
            serve(
                &mut application,
                &mut set,
                &t,
                Duration::from_millis(1),
                shutdown,
            ),
            poller,
        );
        res.unwrap();
        assert_eq!(
            t.deleted.lock().unwrap().as_slice(),
            &["doc-a".to_string()],
            "exactly one delete across the run (cycle 1 archive, cycle 2 quiet)"
        );
    }
}
