//! Device-agnostic on-device deployment: the `DeviceTransport` seam plus the
//! generic publish/sync engine that drives an `App` over any transport. Only
//! keys, PDF bytes, and PDF-space strokes cross this boundary — no reMarkable (or
//! any device) specifics live here.

use std::collections::HashMap;

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
}
