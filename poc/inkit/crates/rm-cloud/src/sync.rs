//! Declarative working-set reconcile — the layer inkapp's loop uses instead of
//! whole-folder pulls. The app declares a target set keyed by app key; we compute the
//! minimal commit and report which keys' doc hash changed since a prior snapshot.

use std::collections::{BTreeMap, BTreeSet};

use crate::client::Client;
use crate::error::Result;
use crate::plumbing::commit::{DocUpsert, Mutation};
use crate::plumbing::snapshot::Snapshot;
use crate::porcelain::docfiles::DocFiles;

/// Metadata key (in `Metadata.extra`) tagging a doc with its app key.
pub const APP_KEY_FIELD: &str = "rmCloudKey";

/// The desired set of app-owned documents, keyed by app key.
///
/// **Destructive semantics:** [`sync`](Client::sync) makes the cloud *match* this set —
/// any app-owned document (one carrying [`APP_KEY_FIELD`]) whose key is **not** present
/// here is **deleted**. An empty `WorkingSet` therefore means "remove all of this app's
/// documents." Pass the full intended set every time; do not pass an empty set as a no-op
/// (use the `since` fast path for that — see [`sync`](Client::sync)).
#[derive(Default)]
pub struct WorkingSet {
    /// app key -> desired document file-set (its metadata must carry `rmCloudKey`).
    pub docs: BTreeMap<String, DocFiles>,
}

/// What `sync` did / observed.
#[derive(Debug, Default)]
pub struct SyncReport {
    /// App keys whose doc hash changed since `since` (ink/content came back).
    pub changed_keys: Vec<String>,
    /// Whether anything was committed.
    pub committed: bool,
}

impl Client {
    /// Reconcile the cloud to `target`. If `since` is given and the generation is
    /// unchanged, returns an empty report immediately (no-op fast path).
    ///
    /// This is **declarative and destructive**: app-owned documents whose key is absent
    /// from `target` are removed (see [`WorkingSet`]). Always pass the complete intended
    /// set; rely on the `since` fast path — not an empty `target` — to express "no change".
    pub async fn sync(
        &self,
        target: WorkingSet,
        since: Option<&Snapshot>,
    ) -> Result<(SyncReport, Snapshot)> {
        let live = self.snapshot().await?;
        if let Some(prev) = since {
            if !prev.root_hash.is_empty() && prev.generation == live.generation {
                return Ok((SyncReport::default(), live));
            }
        }

        // Map existing app-owned docs: app key -> (doc id, doc hash). Reuse the `live`
        // snapshot for each metadata fetch rather than re-snapshotting per doc.
        let mut existing: BTreeMap<String, (String, String)> = BTreeMap::new();
        let live_ids: Vec<(String, String)> = live
            .docs()
            .map(|d| (d.id.clone(), d.hash.clone()))
            .collect();
        for (id, hash) in &live_ids {
            if let Ok(df) = self.get_from(&live, id).await {
                if let Ok(meta) = df.metadata() {
                    if let Some(k) = meta.extra.get(APP_KEY_FIELD).and_then(|v| v.as_str()) {
                        existing.insert(k.to_string(), (id.clone(), hash.clone()));
                    }
                }
            }
        }

        // changed_keys: app keys present now whose doc hash moved vs `since`.
        let mut changed_keys = Vec::new();
        if let Some(prev) = since {
            for (key, (id, _)) in &existing {
                let now = live.doc(id).map(|d| d.hash.clone());
                let before = prev.doc(id).map(|d| d.hash.clone());
                if now != before {
                    changed_keys.push(key.clone());
                }
            }
            changed_keys.sort();
        }

        // Capture the set of targeted app keys before consuming `target.docs`.
        let target_keys: BTreeSet<String> = target.docs.keys().cloned().collect();

        // Upserts: reuse the existing doc id when the key already exists (stable identity).
        let mut upserts = Vec::new();
        for (key, mut df) in target.docs {
            if let Some((id, _)) = existing.get(&key) {
                df.id = id.clone();
            }
            upserts.push(DocUpsert {
                id: df.id.clone(),
                files: df.files,
            });
        }

        // Removals: app-owned docs whose key is no longer in the target.
        let removals: Vec<String> = existing
            .iter()
            .filter(|(k, _)| !target_keys.contains(*k))
            .map(|(_, (id, _))| id.clone())
            .collect();

        let committed = !upserts.is_empty() || !removals.is_empty();
        let snap = if committed {
            self.commit(Mutation { upserts, removals }).await?
        } else {
            live
        };
        Ok((
            SyncReport {
                changed_keys,
                committed,
            },
            snap,
        ))
    }
}
