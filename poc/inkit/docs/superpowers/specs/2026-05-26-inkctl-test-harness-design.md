# inkctl — agent-drivable test harness for inkapp / reMarkable

**Status:** design approved 2026-05-26
**Owner:** Dan
**Scope:** test infrastructure only — no app-facing surface changes

## Summary

A CLI (`inkctl`) and an extended `inkapp-harness` library that together give Claude an agent-drivable interface for exercising inkapp apps end-to-end without hardware — analogous to how Claude uses `playwright-cli` to drive a real browser. The same engine produces durable, committed Rust `#[test]`s: an interactive session can be emitted as a `cargo test`-able file that bypasses the CLI and calls the harness library directly.

## Goals

1. Give Claude an agent-drivable interface to exercise inkapp apps end-to-end without hardware: publish a doc, observe its regions/links/layers, synthesize ink (taps, swipes, real-fixture replay, freeform paths), step the app loop, sync via a realistic fake cloud, and inspect every output (rendered page, layer view, manifest, msg trace, connector writes, secrets accessed, rmdoc bundle contents).
2. Make committed tests fall out of those sessions: an interactive Claude session can be emitted as a `#[test]` against the existing `inkapp-harness` library API, runnable under `cargo test` at native speed.
3. Use the *same* engine for both — the CLI is a thin shell over a public harness API; anything the CLI does, a Rust test can do via the same calls.

## Non-goals

1. **No new app-facing surface.** This is purely test infrastructure. App authors keep using `inkapp` the same way; the `Component` trait and friends are unchanged.
2. **No reMarkable-isms in app-side tests.** Region-targeted gestures stay device-neutral; rm-layer / rm-file inspection is exposed only on the test surface, not in the app's `Component` trait.
3. **Not a replacement for the live `rm-cloud` test suite.** L3 (real cloud) stays gated; this tool is L2 (in-process fake) by default.
4. **No GUI / TUI.** Stdout JSON + PNG files on disk. Agent-readable, scriptable, diffable.
5. **No new transport.** Reuses `rm-cloud`'s existing axum-based `fake` feature and `Snapshot` / `commit` API.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  crates/inkctl  (new)                                       │
│    • clap CLI: parses args, formats JSON/PNG output         │
│    • session dir I/O ($INKCTL_HOME/<session-id>/)           │
│    • thin wrapper — no domain logic                         │
└─────────────────────────────┬───────────────────────────────┘
                              │ public API calls
┌─────────────────────────────▼───────────────────────────────┐
│  crates/inkapp-harness  (extended)                          │
│    • Session: device + fake cloud + open docs               │
│    • Observer: manifest/links/layers/msg-trace/rmdoc tree   │
│    • Synthesizer: tap/swipe/fixture/freeform, per layer     │
│    • Recorder: structured trace → Rust-test emitter         │
│    • Inspector (existing) extended for layer/link overlays  │
└──────┬──────────────────┬──────────────────┬────────────────┘
       │                  │                  │
┌──────▼─────┐  ┌─────────▼────────┐  ┌──────▼──────────────┐
│ inkapp-core│  │ rm-device        │  │ rm-cloud (fake feat)│
│ runtime,   │  │ Device impl +    │  │ in-process axum     │
│ manifest,  │  │ ink coord xform  │  │ blob store + CAS    │
│ readback   │  │                  │  │                     │
└────────────┘  └──────────────────┘  └─────────────────────┘
```

### Architectural rules

- **CLI owns no logic.** `inkctl` is `clap` + JSON serialization + session-dir I/O. Every command is a few lines that calls into `inkapp_harness::session::*`. This is what lets generated `#[test]`s skip the CLI entirely.
- **Session is a directory.** `$INKCTL_HOME/<session-id>/` contains:
  - `cloud/` — fake-cloud blob tree (same layout `rm-cloud`'s fake already uses)
  - `devices/<device-id>/` — paired identity, sync cursor
  - `docs/<doc-id>/` — last-published PDF + manifest snapshot for fast `page describe`
  - `cassettes/` — connector cassette files referenced by `device new --cassette`
  - `trace.jsonl` — append-only command/result log for recording
  - `debug/` — inspector PNGs written by `session step --debug` and failure paths

  Inspecting state = `ls`-ing a directory; no daemon.

- **Engine is in `inkapp-harness`, not in `inkctl`.** Three new modules:
  - `session.rs` — lifecycle, state-dir I/O
  - `observe.rs` — manifest/links/layers/msg/rmdoc views
  - `emit.rs` — trace → Rust `#[test]` code generation

  The existing `simulator.rs`, `inspector.rs`, `recording.rs`, `fixtures.rs` keep their roles; the new modules consume them.

- **No new device seam.** L2 fake cloud is reached through the existing `DeviceTransport` trait; `rm-device::CloudTransport` is configured to point at a session-local axum fake instead of the real reMarkable cloud. L3 swaps the same trait impl for the live one — gated by `--backend=cloud-live`.

- **No new framework dependencies in apps.** Apps never depend on `inkapp-harness` or `inkctl`. The harness depends on `inkapp` (the facade), not the other way around.

### Sync fidelity levels

- **L2 — In-process fake cloud (default).** Session boots the axum-based fake from `rm-cloud`'s `fake` feature in-process. Full HTTP, real CAS-by-generation semantics, supports multiple devices per session for conflict-resolution tests.
- **L3 — Live cloud (gated).** `device new --backend=cloud-live` pairs against the real reMarkable cloud using the existing pairing infra. All docs namespaced under `rmrs-test/<session-id>/`; `session destroy` purges by namespace. Slow, env-gated, never the default.

L1 (in-memory hashmap) is explicitly *not* offered — L2 has the same speed envelope once in-process and gives strictly more coverage.

### Concurrency

One process per CLI call. Session dirs use file locks (`fs2`) so two simultaneous calls against the same session serialize. Different sessions never contend.

### Failure model

Every command returns `{ ok: true, data: ... }` or `{ ok: false, error: { kind, message, context } }` to stdout. Process exit code mirrors `ok`. Claude reads JSON; humans pipe to `jq`.

## Noun set & command surface

Five top-level nouns. Every command is `inkctl <noun> <verb> [args] [--session <id>]`. JSON to stdout unless `--out <path>` is given for binary outputs (PNGs, PDFs, rmdocs).

### `session` — lifecycle

| command | does |
|---|---|
| `session new [--backend=fake\|cloud-live] [--name <n>]` | creates `$INKCTL_HOME/<id>/`, starts in-process axum fake (default) or pairs against live cloud. Prints `{ session_id }`. |
| `session list`                                          | sessions on disk with backend, age, device count. |
| `session destroy <id>`                                  | rm -rf the dir, stop fake cloud; in cloud-live mode also purges `rmrs-test/<id>/` namespace. |
| `session env <id>`                                      | prints `INKCTL_SESSION=<id>` for shell `eval`. |
| `session step [--device <id>] [--debug]`                | runs `App::step` against the device's pending ink. Returns msg trace + model diff + connector writes + secrets read + changed pages + new version. With `--debug`, also writes an inspector PNG per changed page. |

### `device` — virtual reMarkable

| command | does |
|---|---|
| `device new [--name <n>] [--cassette <path>]` | adds a paired device under the session, optionally bound to a connector cassette. Prints `{ device_id }`. |
| `device list`                                  | devices in the session, with sync cursor. |
| `device tree <id> [--path <p>]`                | rmdoc bundle tree on this device (mirrors `rmapi ls`), with file type, parent, last sync. |
| `device sync <id>`                             | runs one push+pull cycle against the session's fake cloud. Returns `{ pushed, pulled, conflicts }`. |

### `document` — published app docs

| command | does |
|---|---|
| `document publish <device-id> <app-path> [--config <deploy.toml>]` | compiles the app, runs its publish path, syncs to the device. Prints `{ doc_id, pages, version }`. |
| `document open <doc-id>`                                           | marks doc as "current" for the session. |
| `document describe <doc-id>`                                       | manifest summary: pages, version, app state JSON, region count per page, links count per page, version history. |
| `document pdf <doc-id> --out <path>`                               | dumps the current PDF to disk. |
| `document rmdoc <doc-id> --out <path>`                             | dumps the rmdoc bundle (PDF + .content + .metadata + any .rm scenes). |

### `page` — a single page of a document

| command | does |
|---|---|
| `page describe <doc-id> <page>`                                                                       | the accessibility tree: regions (name, rect, app-state), links (rect, target), layers present, ink summary per region. Primary "what's here" view for Claude. |
| `page snapshot <doc-id> <page> --out <png>`                                                           | rendered page PNG. |
| `page inspect <doc-id> <page> [--layers <a,b>] [--show=regions,links,strokes,attributed] --out <png>` | layer inspector: color-codes regions, link targets, synth strokes, attributed strokes. Default debug output on any failure. |
| `page links <doc-id> <page>`                                                                          | just the link table. |

### `ink` — strokes (synthesis + readback)

| command | does |
|---|---|
| `ink tap <doc-id> <page> <region> [--layer=pen]`                                       | synthesize a single dot in the region center. |
| `ink swipe <doc-id> <page> <region> [--layer=highlights]`                              | horizontal highlight across region width. |
| `ink fixture <doc-id> <page> <region> <fixture-name>`                                  | replay recorded ink from `tests/fixtures/gestures/<name>.json`. |
| `ink draw <doc-id> <page> --path "x,y x,y …" [--layer=<l>] [--highlighter]`            | freeform polyline; PDF coords by default, `--device-space` flag for transform-fidelity tests. |
| `ink list <doc-id> <page> [--by-layer] [--by-region]`                                  | readback view: every stroke currently on the page. |
| `link follow <doc-id> <page> <region>`                                                 | high-level "tap and follow link" — resolves the link in the region, sets session's current page to the target. |

### `record` / `replay` — debugging side-channel (not test format)

| command | does |
|---|---|
| `record start`                                          | begin appending to `trace.jsonl`. |
| `record stop --out <path>`                              | finalize trace file. |
| `record assert <field>=<value>`                         | annotate trace with an assertion to bake into emitted tests. |
| `replay <trace-path>`                                   | re-run commands in order against the current session. |
| `emit test --from <trace> --out tests/<name>.rs`        | trace → Rust `#[test]` generator against `inkapp_harness::session::*`. |

## Observation surface (concrete shapes)

Two JSON shapes Claude reads most often.

**`page describe` output:**

```json
{
  "doc_id": "doc-1", "page": 2, "version": 7,
  "regions": [
    { "name": "index.row.3", "rect": [12, 340, 408, 380],
      "layer_hint": "pen",
      "link": { "target": "page:14" },
      "app_state": { "selected": false },
      "ink": { "strokes": 1, "by_layer": { "Layer 1": 1 } } }
  ],
  "links": [ { "rect": [12, 340, 408, 380], "target": "page:14", "region": "index.row.3" } ],
  "layers_present": ["Layer 1", "Highlights"],
  "image": "page-2.png"
}
```

**`session step` output:**

```json
{
  "cycle": 4,
  "msgs": [ { "type": "Open", "row": 3 } ],
  "model_diff": { "selected_row": [null, 3], "open_doc": [null, "art-42"] },
  "connector_writes": [ { "connector": "readwise", "op": "mark_read", "id": "art-42" } ],
  "secrets_read": [ { "store": "readwise", "key": "api_token" } ],
  "pages_changed": [2, 14],
  "new_version": 8,
  "debug_renders": ["debug/cycle-4-page-2.png", "debug/cycle-4-page-14.png"]
}
```

Every other observation (`device tree`, `ink list`, `page links`, `document describe`) is a strict subset/sibling of these.

## Recording → Rust test generation

**Trace format** (`trace.jsonl`): one JSON object per CLI call.

- Command entry: `{ ts, kind: "call", cmd: ["page","describe","doc-1","2"], args: {...}, result: {...} }`
- Assertion entry (from `record assert`): `{ ts, kind: "assert", target: "<json-path-into-last-result>", expected: <value> }`. The emitter binds each assertion to the immediately preceding call's result.

**Emitter** (`inkapp_harness::emit::to_rust`): walks the trace, produces a self-contained `#[test]` like:

```rust
#[test]
fn reading_queue_opens_article_on_tap() {
    let mut s = inkapp_harness::Session::new_fake();
    let dev = s.device_new("rm");
    let doc = s.document_publish(&dev, "apps/reading-queue").unwrap();
    s.ink_tap(&doc, 2, "index.row.3");
    let step = s.step(&dev);
    assert_eq!(step.msgs, vec![Msg::Open { row: 3 }]);
    assert!(step.pages_changed.contains(&14));
    s.assert_region_app_state(&doc, 2, "index.row.3", json!({ "selected": true }));
}
```

Emitter rules:

- Elides pure-observation calls (`page describe`, `ink list`, `document describe`) — exploratory reads don't belong in committed tests.
- Keeps mutations (`device new`, `document publish`, `ink *`, `session step`, `link follow`).
- Bakes in any `record assert` annotations Claude added mid-session.
- Generated file imports `inkapp_harness` and friends; no subprocess, no JSON, no CLI dependency at runtime.

## Crate layout

```
crates/
  inkapp-harness/          # extended
    src/
      lib.rs
      session.rs           # new — Session, SessionConfig, state-dir I/O
      observe.rs           # new — describe/links/layers/tree views
      emit.rs              # new — trace → Rust #[test] generator
      simulator.rs         # existing
      inspector.rs         # existing; extended for layer/link overlays
      recording.rs         # existing; trace.jsonl writer here
      fixtures.rs          # existing
  inkctl/                  # new
    Cargo.toml
    src/
      main.rs              # clap top-level
      cmd/
        session.rs
        device.rs
        document.rs
        page.rs
        ink.rs
        record.rs
      output.rs            # JSON envelope + PNG writer
```

No new top-level crates beyond `inkctl`. The harness gains three modules but its public role (in-software loop simulator + inspector) is unchanged — it just grows a `Session` facade.

## Testing strategy for the tool itself

- **Harness library tests** (`crates/inkapp-harness/tests/`): exercise `Session` directly without the CLI. Existing `acceptance.rs`, `e2e.rs`, etc. get migrated to `Session` where it simplifies them, but the migration isn't a precondition.
- **CLI smoke tests** (`crates/inkctl/tests/`): spawn `inkctl` with `assert_cmd`, JSON-decode stdout, verify each noun's commands round-trip. ~one test per verb.
- **Dogfood test** (`crates/inkctl/tests/dogfood.rs`): runs a recorded trace against `apps/reading-queue`, emits a Rust test via the generator, then `cargo test`s the emitted file in a `tempdir`. Catches drift between CLI shape, trace format, and emitter.

## Risks & open questions

1. **Fake cloud feature surface drift.** `rm-cloud`'s `fake` feature is currently scoped to `rm-cloud`'s own tests. Promoting it to a session-backing service may surface bugs (CAS edge cases, multi-device contention) we haven't hit. Mitigation: dogfood test + a focused conflict-resolution test in the harness.
2. **Stroke-recording fidelity.** `ink draw --path` synthesizes idealized polylines; real pen input has pressure, tilt, speed, segmentation. Apps shouldn't care, but transform-fidelity tests do. Mitigation: `ink fixture` (real recorded `.rm` data) remains the way to test parsing/transform; `draw` is for region-attribution tests where geometry is what matters.
3. **Secret leakage into traces.** `secrets_read` reports *which* secrets were read but never values. Enforce in the observer — if a value ever shows up in a trace JSON, that's a test-suite bug. Add a regression test that grep-asserts no secret-named tokens appear in any committed trace.
4. **L3 (live cloud) parity.** When run with `--backend=cloud-live`, sessions can leak state into the real reMarkable account if cleanup fails. Mitigation: namespace under `rmrs-test/<session-id>/` (matches existing `rm-cloud` live-test convention); `session destroy` purges by namespace.
5. **Open: connector cassette format.** Readwise has a cassette mode; ICS/localcal don't. Out of scope for v1 — just plumb `--cassette` through and let each connector decide. Tracked as a follow-up.

## Definition of done

Per the project convention, this spec is "built" when:

1. `crates/inkctl/` exists and the full command surface above works against `apps/reading-queue` and `apps/agenda`.
2. `inkapp_harness::Session` exists and has at least one harness-level test exercising every public method.
3. The dogfood test in `crates/inkctl/tests/dogfood.rs` passes.
4. `docs/appdx.md` is updated to mark the test-harness item built and reflect the new surface.
