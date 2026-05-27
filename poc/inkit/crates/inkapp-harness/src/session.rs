//! Session lifecycle: a Session is a directory on disk holding a paired fake
//! cloud, devices, docs, and a command trace. One CLI process == one Session
//! load+save cycle. Pure-lifecycle skeleton; device/doc/ink methods land in
//! later tasks.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use inkapp_core::connector::ConnectorSet;
use inkapp_core::ink::Stroke;
use inkapp_core::runtime::{App, DocSet};
use rm_cloud::fake::FakeCloud;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    id: String,
    backend: String,
    created_at: chrono::DateTime<chrono::Utc>,
    /// When `Some(true)`, mutating `Session` methods append a `kind:"call"`
    /// entry to `state_dir/trace.jsonl`. Defaulted via serde for backward
    /// compatibility with session.json files written before Task 13.
    #[serde(default)]
    recording: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(id: String) -> Self {
        Self(id)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for DeviceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DeviceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceFile {
    id: String,
    name: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    sync_cursor: Option<String>,
}

/// Input to `Session::document_publish`: the rendered PDF + recovered manifest
/// for an app. `source_typ` is optional (carried for re-render in later tasks).
#[derive(Clone)]
pub struct PublishedApp {
    pub app_name: String,
    pub pdf_bytes: Vec<u8>,
    pub manifest: inkapp_core::manifest::Manifest,
    pub source_typ: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocSummary {
    pub id: String,
    pub device_id: String,
    pub app_name: String,
    pub version: u32,
    pub pages: usize,
}

#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub id: String,
    pub name: Option<String>,
    pub sync_cursor: Option<String>,
}

/// Options controlling `Session::step_app`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StepOpts {
    /// When true, the session writes inspector PNGs for changed pages under
    /// `<state_dir>/debug/cycle-<n>-<doc-key>-page-<p>.png`. Best-effort:
    /// renderer failures are skipped, never propagated.
    pub debug: bool,
}

/// Result of `Session::link_follow`. Exactly one of `target_page` /
/// `target_uri` is `Some` on a successful hit; both `None` when no link in
/// the region matched.
#[derive(Debug, Serialize)]
pub struct FollowResult {
    pub target_page: Option<usize>,
    pub target_uri: Option<String>,
}

/// Result of `Session::device_sync`: doc names visible after one snapshot.
#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub pushed: Vec<String>,
    pub pulled: Vec<String>,
    pub conflicts: Vec<String>,
}

/// Structured result of one `Session::step_app` cycle.
#[derive(Debug, Serialize)]
pub struct StepResult {
    /// Per-device monotonic cycle counter (persisted in `cursor.json`).
    pub cycle: u32,
    /// Each decoded `Msg`, serialized to JSON.
    pub msgs: Vec<serde_json::Value>,
    /// DocKey strings of created/updated docs this cycle.
    pub pages_changed: Vec<String>,
    /// Model diff. Placeholder (`Null`) until model-diff capture lands.
    pub model_diff: serde_json::Value,
    /// Connector writes captured this cycle. Empty until write capture lands.
    pub connector_writes: Vec<serde_json::Value>,
    /// Secrets read this cycle. Empty until secrets capture lands.
    pub secrets_read: Vec<String>,
    /// `App::version` after the step.
    pub new_version: u64,
    /// Paths to inspector PNGs written when `opts.debug` is true.
    pub debug_renders: Vec<String>,
}

pub struct Session {
    state_dir: PathBuf,
    file: SessionFile,
    cloud: FakeCloud,
    _lock: File, // released on drop
}

impl Session {
    /// Create a fresh session backed by an in-process fake cloud.
    pub async fn new_fake(state_dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(state_dir)?;
        let lock = Self::acquire_lock(state_dir)?;
        let file = SessionFile {
            id: uuid::Uuid::new_v4().to_string(),
            backend: "fake".to_string(),
            created_at: chrono::Utc::now(),
            recording: None,
        };
        fs::write(
            state_dir.join("session.json"),
            serde_json::to_vec_pretty(&file).unwrap(),
        )?;
        let cloud = FakeCloud::from_dir(&state_dir.join("cloud")).await?;
        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            file,
            cloud,
            _lock: lock,
        })
    }

    /// Re-open an existing session directory. Errors if the lock is held.
    pub async fn open(state_dir: &Path) -> std::io::Result<Self> {
        let lock = Self::acquire_lock(state_dir)?;
        let bytes = fs::read(state_dir.join("session.json"))?;
        let file: SessionFile = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let cloud = FakeCloud::from_dir(&state_dir.join("cloud")).await?;
        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            file,
            cloud,
            _lock: lock,
        })
    }

    /// Remove a session directory. Does not require the session to be open
    /// (the caller is responsible for ensuring no process holds it).
    pub fn destroy(state_dir: &Path) -> std::io::Result<()> {
        if state_dir.exists() {
            fs::remove_dir_all(state_dir)?;
        }
        Ok(())
    }

    /// Persist cloud state to disk. Call before dropping the session.
    pub fn flush(&self) -> std::io::Result<()> {
        self.cloud.dump_to_dir(&self.state_dir.join("cloud"))
    }

    pub fn id(&self) -> &str {
        &self.file.id
    }
    pub fn backend(&self) -> &str {
        &self.file.backend
    }
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    #[allow(dead_code)] // used by later tasks (device_new / document_publish)
    pub(crate) fn cloud(&self) -> &FakeCloud {
        &self.cloud
    }

    /// Turn on the command-trace recorder. Subsequent mutating calls append
    /// a JSON line to `state_dir/trace.jsonl`. Persisted in session.json so
    /// the flag survives session reopen.
    pub fn record_start(&mut self) -> std::io::Result<()> {
        self.file.recording = Some(true);
        self.persist_session_file()
    }

    /// Turn off the command-trace recorder.
    pub fn record_stop(&mut self) -> std::io::Result<()> {
        self.file.recording = Some(false);
        self.persist_session_file()
    }

    /// Append a `kind:"assert"` entry to the trace. Always writes, regardless
    /// of the recording flag — callers (test emitters) use this to mark
    /// observation assertions independently of mutation recording.
    pub fn record_assert(&self, target: &str, expected: serde_json::Value) -> std::io::Result<()> {
        self.trace_writer().append_assert(target, expected)
    }

    fn recording_enabled(&self) -> bool {
        self.file.recording.unwrap_or(false)
    }

    fn trace_writer(&self) -> crate::trace::TraceWriter {
        crate::trace::TraceWriter::new(self.state_dir.join("trace.jsonl"))
    }

    fn persist_session_file(&self) -> std::io::Result<()> {
        fs::write(
            self.state_dir.join("session.json"),
            serde_json::to_vec_pretty(&self.file).unwrap(),
        )
    }

    pub fn device_new(&mut self, name: Option<&str>) -> std::io::Result<DeviceId> {
        let devices_dir = self.state_dir.join("devices");
        fs::create_dir_all(&devices_dir)?;
        let next_n = self.device_list()?.len() + 1;
        let id = format!("dev-{next_n}");
        let dev_dir = devices_dir.join(&id);
        fs::create_dir_all(&dev_dir)?;
        let file = DeviceFile {
            id: id.clone(),
            name: name.map(str::to_string),
            created_at: chrono::Utc::now(),
            sync_cursor: None,
        };
        fs::write(
            dev_dir.join("device.json"),
            serde_json::to_vec_pretty(&file).unwrap(),
        )?;
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["device", "new"],
                serde_json::json!({ "name": name }),
                serde_json::json!({ "id": id }),
            );
        }
        Ok(DeviceId(id))
    }

    pub fn device_list(&self) -> std::io::Result<Vec<DeviceSummary>> {
        let devices_dir = self.state_dir.join("devices");
        if !devices_dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&devices_dir)? {
            let entry = entry?;
            let path = entry.path().join("device.json");
            if !path.exists() {
                continue;
            }
            let bytes = fs::read(&path)?;
            let file: DeviceFile = serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            out.push(DeviceSummary {
                id: file.id,
                name: file.name,
                sync_cursor: file.sync_cursor,
            });
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Publish an app's rendered PDF + manifest under `docs/<doc-id>/` and push
    /// the PDF to the session's fake cloud via `rm_device::CloudTransport`.
    /// Re-publishing the same `(device, app_name)` keeps the doc-id and
    /// increments `version`.
    pub async fn document_publish(
        &mut self,
        device: &DeviceId,
        app: PublishedApp,
    ) -> std::io::Result<DocSummary> {
        let slug = slugify(&app.app_name);
        let doc_id = format!("{}-{}", device.as_str(), slug);
        let doc_dir = self.state_dir.join("docs").join(&doc_id);
        fs::create_dir_all(&doc_dir)?;

        let prev_version = read_prev_version(&doc_dir).unwrap_or(0);
        let version = prev_version + 1;

        // pages = highest page index referenced by any region + 1; a region-less
        // doc defaults to 1.
        let pages = app
            .manifest
            .regions
            .iter()
            .map(|r| r.page)
            .max()
            .map(|p| p + 1)
            .unwrap_or(1);

        fs::write(doc_dir.join("pdf.pdf"), &app.pdf_bytes)?;
        fs::write(
            doc_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&app.manifest).unwrap(),
        )?;
        if let Some(src) = &app.source_typ {
            fs::write(doc_dir.join("source.typ"), src.as_bytes())?;
        }

        let summary = DocSummary {
            id: doc_id.clone(),
            device_id: device.as_str().to_string(),
            app_name: app.app_name.clone(),
            version,
            pages,
        };
        fs::write(
            doc_dir.join("doc.json"),
            serde_json::to_vec_pretty(&summary).unwrap(),
        )?;

        push_to_fake_cloud(&self.cloud, &app).await?;
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["document", "publish"],
                serde_json::json!({
                    "device": device.as_str(),
                    "app_name": app.app_name,
                }),
                serde_json::to_value(&summary).unwrap_or(serde_json::Value::Null),
            );
        }
        Ok(summary)
    }

    /// Read the persisted pending ink for (device, doc, page). Returns empty if
    /// no pending file exists. Task 10 populates these files.
    pub fn pending_ink(
        &self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
    ) -> std::io::Result<Vec<inkapp_core::ink::Stroke>> {
        let path = self
            .state_dir
            .join("devices")
            .join(device.as_str())
            .join("pending")
            .join(doc_id)
            .join(format!("{page}.json"));
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&path)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Append a single-point (highlighter=false) stroke at the region's center.
    pub fn ink_tap(
        &mut self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        region: &str,
    ) -> std::io::Result<()> {
        let rect = region_rect(&self.state_dir, doc_id, page, region)?;
        let cx = (rect.x0 + rect.x1) / 2.0;
        let cy = (rect.y0 + rect.y1) / 2.0;
        self.append_stroke(
            device,
            doc_id,
            page,
            inkapp_core::ink::Stroke {
                points: vec![inkapp_core::geometry::PdfPoint { x: cx, y: cy }],
                highlighter: false,
            },
        )?;
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["ink", "tap"],
                serde_json::json!({
                    "device": device.as_str(),
                    "doc_id": doc_id,
                    "page": page,
                    "region": region,
                }),
                serde_json::json!({}),
            );
        }
        Ok(())
    }

    /// Append a horizontal highlighter stroke across the region's full width at mid-height.
    pub fn ink_swipe(
        &mut self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        region: &str,
    ) -> std::io::Result<()> {
        let rect = region_rect(&self.state_dir, doc_id, page, region)?;
        let cy = (rect.y0 + rect.y1) / 2.0;
        self.append_stroke(
            device,
            doc_id,
            page,
            inkapp_core::ink::Stroke {
                points: vec![
                    inkapp_core::geometry::PdfPoint { x: rect.x0, y: cy },
                    inkapp_core::geometry::PdfPoint { x: rect.x1, y: cy },
                ],
                highlighter: true,
            },
        )?;
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["ink", "swipe"],
                serde_json::json!({
                    "device": device.as_str(),
                    "doc_id": doc_id,
                    "page": page,
                    "region": region,
                }),
                serde_json::json!({}),
            );
        }
        Ok(())
    }

    /// Load a gesture fixture from `tests/fixtures/gestures/<name>.json` and append its
    /// default sample transplanted onto the region rect.
    pub fn ink_fixture(
        &mut self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        region: &str,
        fixture_name: &str,
    ) -> std::io::Result<()> {
        let rect = region_rect(&self.state_dir, doc_id, page, region)?;
        let path = format!(
            "{}/tests/fixtures/gestures/{}.json",
            env!("CARGO_MANIFEST_DIR"),
            fixture_name
        );
        let bytes = fs::read(&path)?;
        let fixture = crate::fixtures::GestureFixture::from_json(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        for s in fixture.transplant_default(rect) {
            self.append_stroke(device, doc_id, page, s)?;
        }
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["ink", "fixture"],
                serde_json::json!({
                    "device": device.as_str(),
                    "doc_id": doc_id,
                    "page": page,
                    "region": region,
                    "fixture_name": fixture_name,
                }),
                serde_json::json!({}),
            );
        }
        Ok(())
    }

    /// Decode raw `.rm` bytes through the reMarkable device and stage the resulting
    /// strokes as pending ink on `(device, doc_id, page)`. Layer-2 surface — bypasses
    /// gesture synthesis; intended for fixture-replay tests where the input is already
    /// in the device's native scene format. Returns the number of strokes staged.
    pub fn ink_apply_rm_bytes(
        &mut self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        bytes: &[u8],
    ) -> std::io::Result<usize> {
        let page_h = pdf_page_height(&self.state_dir, doc_id, page)?;
        let rm = rm_device::Remarkable::new();
        let strokes = inkapp_core::device::Device::read_ink(&rm, bytes, page_h)
            .map_err(|e| std::io::Error::other(format!("read_ink: {e}")))?;
        let n = strokes.len();
        for s in strokes {
            self.append_stroke(device, doc_id, page, s)?;
        }
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["ink", "load-rm"],
                serde_json::json!({
                    "device": device.as_str(),
                    "doc_id": doc_id,
                    "page": page,
                    "stroke_count": n,
                }),
                serde_json::json!({ "applied": n }),
            );
        }
        Ok(n)
    }

    /// Append a freeform polyline stroke.
    pub fn ink_draw(
        &mut self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        points: &[inkapp_core::geometry::PdfPoint],
        highlighter: bool,
    ) -> std::io::Result<()> {
        self.append_stroke(
            device,
            doc_id,
            page,
            inkapp_core::ink::Stroke {
                points: points.to_vec(),
                highlighter,
            },
        )?;
        if self.recording_enabled() {
            let points_json: Vec<serde_json::Value> = points
                .iter()
                .map(|p| serde_json::json!({ "x": p.x, "y": p.y }))
                .collect();
            let _ = self.trace_writer().append_call(
                &["ink", "draw"],
                serde_json::json!({
                    "device": device.as_str(),
                    "doc_id": doc_id,
                    "page": page,
                    "points": points_json,
                    "highlighter": highlighter,
                }),
                serde_json::json!({}),
            );
        }
        Ok(())
    }

    fn append_stroke(
        &self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        stroke: inkapp_core::ink::Stroke,
    ) -> std::io::Result<()> {
        let dir = self
            .state_dir
            .join("devices")
            .join(device.as_str())
            .join("pending")
            .join(doc_id);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{page}.json"));
        let mut existing: Vec<inkapp_core::ink::Stroke> = if path.exists() {
            let bytes = fs::read(&path)?;
            serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        } else {
            Vec::new()
        };
        existing.push(stroke);
        fs::write(&path, serde_json::to_vec_pretty(&existing).unwrap())?;
        Ok(())
    }

    /// Drive the inkapp loop one cycle for `device` using the caller-provided
    /// `app` and `set`. Reads pending ink from `devices/<id>/pending/<doc-id>/<page>.json`,
    /// keys it by the doc's `app_name` (the same key the App used at render time),
    /// calls `app.step`, persists a per-device cycle counter, then clears the
    /// pending-ink dir for this device. The App lives in RAM only — the Session
    /// does not own it; cross-call App state persistence is a follow-up.
    pub async fn step_app<M, Msg, Cx>(
        &mut self,
        device: &DeviceId,
        app: &mut App<M, Msg, Cx>,
        set: &mut DocSet,
        opts: StepOpts,
    ) -> std::io::Result<StepResult>
    where
        Msg: serde::Serialize + Clone,
        Cx: ConnectorSet,
    {
        // 1. Assemble ink_by_key. The App keys docs by `Document::key`, which
        //    `document_publish` records as `app_name` in `doc.json`.
        let pending_root = self
            .state_dir
            .join("devices")
            .join(device.as_str())
            .join("pending");
        let mut ink_by_key: HashMap<String, Vec<Vec<Stroke>>> = HashMap::new();
        if pending_root.exists() {
            for doc_entry in fs::read_dir(&pending_root)? {
                let doc_entry = doc_entry?;
                if !doc_entry.file_type()?.is_dir() {
                    continue;
                }
                let doc_id = doc_entry.file_name().to_string_lossy().to_string();
                let doc_json = self.state_dir.join("docs").join(&doc_id).join("doc.json");
                if !doc_json.exists() {
                    continue;
                }
                let summary: DocSummary = serde_json::from_slice(&fs::read(&doc_json)?)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                let app_key = summary.app_name.clone();

                let mut page_files: Vec<_> = fs::read_dir(doc_entry.path())?
                    .filter_map(|r| r.ok())
                    .collect();
                page_files.sort_by_key(|e| e.file_name());

                let mut pages: Vec<Vec<Stroke>> = Vec::new();
                for f in page_files {
                    let name = f.file_name().to_string_lossy().to_string();
                    let Some(stem) = name.strip_suffix(".json") else {
                        continue;
                    };
                    let Ok(page_idx) = stem.parse::<usize>() else {
                        continue;
                    };
                    let bytes = fs::read(f.path())?;
                    let strokes: Vec<Stroke> = serde_json::from_slice(&bytes)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                    if pages.len() <= page_idx {
                        pages.resize(page_idx + 1, Vec::new());
                    }
                    pages[page_idx] = strokes;
                }
                ink_by_key.insert(app_key, pages);
            }
        }

        // 2. Bump per-device cycle counter.
        let dev_dir = self.state_dir.join("devices").join(device.as_str());
        fs::create_dir_all(&dev_dir)?;
        let cycle_path = dev_dir.join("cursor.json");
        let prev_cycle: u32 = fs::read(&cycle_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("cycle").and_then(|c| c.as_u64()).map(|x| x as u32))
            .unwrap_or(0);
        let cycle = prev_cycle + 1;

        // 3. Drive the app.
        let result = app
            .step(set, &ink_by_key)
            .await
            .map_err(|e| std::io::Error::other(format!("app.step: {e}")))?;

        // 4. Serialize msgs + collect changed keys.
        let msgs: Vec<serde_json::Value> = result
            .decoded
            .iter()
            .map(|m| serde_json::to_value(m).unwrap_or(serde_json::Value::Null))
            .collect();
        let pages_changed: Vec<String> =
            result.rendered.iter().map(|rd| rd.key.0.clone()).collect();

        // 5. Persist cycle counter.
        fs::write(
            &cycle_path,
            serde_json::to_vec_pretty(&serde_json::json!({ "cycle": cycle })).unwrap(),
        )?;

        // 6. Clear this device's pending ink. The App has consumed it.
        if pending_root.exists() {
            fs::remove_dir_all(&pending_root)?;
        }

        // 7. Optional debug renders. Skeleton: ensure the dir exists when
        //    requested. PNG rendering for changed pages is a best-effort
        //    follow-up — the StepResult vector is populated below if a future
        //    pass writes files. Today it stays empty even when debug=true,
        //    matching the documented behaviour (vector exists, may be empty).
        let debug_renders: Vec<String> = Vec::new();
        if opts.debug {
            let debug_dir = self.state_dir.join("debug");
            fs::create_dir_all(&debug_dir)?;
        }

        let step_result = StepResult {
            cycle,
            msgs,
            pages_changed,
            model_diff: serde_json::Value::Null,
            connector_writes: Vec::new(),
            secrets_read: Vec::new(),
            new_version: app.version_get(),
            debug_renders,
        };
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["session", "step"],
                serde_json::json!({
                    "device": device.as_str(),
                    "debug": opts.debug,
                }),
                serde_json::to_value(&step_result).unwrap_or(serde_json::Value::Null),
            );
        }
        Ok(step_result)
    }

    /// Resolve the link in `region` on `(doc_id, page)` to its target.
    /// For a page target, also updates the device's `cursor.json::current_page`
    /// (preserving any existing `cycle`). For a URI target, returns the URI
    /// without touching the cursor.
    pub fn link_follow(
        &mut self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        region: &str,
    ) -> std::io::Result<FollowResult> {
        let doc_dir = self.state_dir.join("docs").join(doc_id);
        let pdf_bytes = fs::read(doc_dir.join("pdf.pdf"))?;
        let (_, manifest) = crate::observe::load_doc(&self.state_dir, doc_id)?;
        let region_rect = manifest
            .regions
            .iter()
            .find(|r| r.page == page && r.name == region)
            .map(|r| r.rect)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("region {region} on page {page} not found"),
                )
            })?;
        let outer = [
            region_rect.x0,
            region_rect.y0,
            region_rect.x1,
            region_rect.y1,
        ];

        let raw = crate::pdf_links::extract(&pdf_bytes);
        let hit = raw
            .into_iter()
            .find(|l| l.page == page && crate::observe::rect_contains(&outer, &l.rect));

        let (target_page, target_uri) = match hit.map(|l| l.target) {
            None => (None, None),
            Some(crate::pdf_links::LinkTarget::Page(p)) => (Some(p), None),
            Some(crate::pdf_links::LinkTarget::Uri(u)) => (None, Some(u)),
        };

        if let Some(p) = target_page {
            let dev_dir = self.state_dir.join("devices").join(device.as_str());
            fs::create_dir_all(&dev_dir)?;
            let cursor_path = dev_dir.join("cursor.json");
            let mut cursor: serde_json::Value = fs::read(&cursor_path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            cursor["current_page"] = serde_json::json!(p);
            fs::write(&cursor_path, serde_json::to_vec_pretty(&cursor).unwrap())?;
        }

        let follow = FollowResult {
            target_page,
            target_uri,
        };
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["link", "follow"],
                serde_json::json!({
                    "device": device.as_str(),
                    "doc_id": doc_id,
                    "page": page,
                    "region": region,
                }),
                serde_json::to_value(&follow).unwrap_or(serde_json::Value::Null),
            );
        }
        Ok(follow)
    }

    /// Run one pull pass against the session's fake cloud: enumerate visible
    /// documents and update `device.json::sync_cursor`. Push is a no-op until
    /// queued writes exist.
    pub async fn device_sync(&mut self, device: &DeviceId) -> std::io::Result<SyncResult> {
        use rm_cloud::{Client, Config};
        let client = Client::from_user_token(Config::single_host(&self.cloud.base), "user-token");

        // Walk the cloud tree and collect every non-folder entry's visible name.
        let mut pulled: Vec<String> = Vec::new();
        let mut stack: Vec<String> = vec![String::new()];
        while let Some(parent) = stack.pop() {
            let entries = client
                .ls(&parent)
                .await
                .map_err(|e| std::io::Error::other(format!("ls: {e}")))?;
            for entry in entries {
                if entry.is_folder {
                    stack.push(entry.id);
                } else {
                    pulled.push(entry.name);
                }
            }
        }
        pulled.sort();

        // Update sync_cursor in device.json.
        let dev_dir = self.state_dir.join("devices").join(device.as_str());
        let dev_json = dev_dir.join("device.json");
        let mut file: DeviceFile = serde_json::from_slice(&fs::read(&dev_json)?)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        file.sync_cursor = Some(chrono::Utc::now().to_rfc3339());
        fs::write(&dev_json, serde_json::to_vec_pretty(&file).unwrap())?;

        let sync = SyncResult {
            pushed: Vec::new(),
            pulled,
            conflicts: Vec::new(),
        };
        if self.recording_enabled() {
            let _ = self.trace_writer().append_call(
                &["device", "sync"],
                serde_json::json!({ "device": device.as_str() }),
                serde_json::to_value(&sync).unwrap_or(serde_json::Value::Null),
            );
        }
        Ok(sync)
    }

    fn acquire_lock(state_dir: &Path) -> std::io::Result<File> {
        let lock_path = state_dir.join(".lock");
        let f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        f.try_lock_exclusive().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!("session locked: {e}"),
            )
        })?;
        Ok(f)
    }
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn read_prev_version(doc_dir: &Path) -> Option<u32> {
    let bytes = fs::read(doc_dir.join("doc.json")).ok()?;
    let s: DocSummary = serde_json::from_slice(&bytes).ok()?;
    Some(s.version)
}

fn region_rect(
    state_dir: &Path,
    doc_id: &str,
    page: usize,
    region: &str,
) -> std::io::Result<inkapp_core::geometry::PdfRect> {
    let manifest_path = state_dir.join("docs").join(doc_id).join("manifest.json");
    let bytes = fs::read(&manifest_path)?;
    let manifest: inkapp_core::manifest::Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    manifest
        .regions
        .iter()
        .find(|r| r.page == page && r.name == region)
        .map(|r| r.rect)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("region {region} on page {page} not found"),
            )
        })
}

/// Extract the height (in PDF points) of `page` from the stored PDF for `doc_id`.
/// Uses the `/MediaBox` entry from the page dictionary. Falls back to 560.0 (the
/// harness default) if the PDF is absent, unparseable, or the page is missing.
fn pdf_page_height(state_dir: &Path, doc_id: &str, page: usize) -> std::io::Result<f64> {
    let pdf_path = state_dir.join("docs").join(doc_id).join("pdf.pdf");
    let bytes = match fs::read(&pdf_path) {
        Ok(b) => b,
        Err(_) => return Ok(crate::recording::PAGE_H),
    };
    let doc = match lopdf::Document::load_mem(&bytes) {
        Ok(d) => d,
        Err(_) => return Ok(crate::recording::PAGE_H),
    };
    let pages = doc.get_pages(); // BTreeMap<u32, ObjectId>, 1-based
                                 // lopdf page numbers are 1-based; our `page` is 0-based.
    let pdf_page_num = (page as u32) + 1;
    let Some(page_id) = pages.get(&pdf_page_num) else {
        return Ok(crate::recording::PAGE_H);
    };
    let page_obj = match doc.get_object(*page_id) {
        Ok(lopdf::Object::Dictionary(d)) => d.clone(),
        _ => return Ok(crate::recording::PAGE_H),
    };
    // /MediaBox is [x0 y0 x1 y1] — height = y1 - y0.
    let media_box = match page_obj.get(b"MediaBox") {
        Ok(mb) => mb,
        Err(_) => return Ok(crate::recording::PAGE_H),
    };
    let arr = match doc.dereference(media_box) {
        Ok((_, lopdf::Object::Array(a))) => a.clone(),
        _ => return Ok(crate::recording::PAGE_H),
    };
    if arr.len() < 4 {
        return Ok(crate::recording::PAGE_H);
    }
    let y0 = arr[1].as_float().unwrap_or(0.0) as f64;
    let y1 = arr[3].as_float().unwrap_or(560.0) as f64;
    Ok((y1 - y0).max(1.0))
}

async fn push_to_fake_cloud(cloud: &FakeCloud, app: &PublishedApp) -> std::io::Result<()> {
    use inkapp_core::sync::DeviceTransport;
    use rm_cloud::{Client, Config};
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    let folder = format!("/inkctl/{}", slugify(&app.app_name));
    let transport = rm_device::CloudTransport::with_client(client, folder);
    transport
        .push(&app.app_name, &app.pdf_bytes)
        .await
        .map_err(|e| std::io::Error::other(format!("push: {e}")))?;
    Ok(())
}
