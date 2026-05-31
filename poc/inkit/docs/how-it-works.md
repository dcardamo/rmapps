# How inkapp works

## The loop

inkapp is built around one repeating cycle:

```
  ┌────────────────────────────────────────────────────────┐
  │                                                        │
  │  Render (Typst)                                        │
  │    │  Produces a PDF with embedded manifest            │
  │    ▼                                                   │
  │  Sync to device  (rm-cloud / device abstraction)       │
  │    │  PDF lands on reMarkable (or Supernote, etc.)     │
  │    ▼                                                   │
  │  User reads and writes with the pen                    │
  │    │  Annotations stored on-device as .rm files        │
  │    ▼                                                   │
  │  Sync back  (device syncs to cloud)                    │
  │    │  Annotated .rm files are now accessible           │
  │    ▼                                                   │
  │  Readback  (rmfiles crate parses .rm)                  │
  │    │  Ink strokes mapped to named page regions         │
  │    ▼                                                   │
  │  Process  (handler: Rust code, optional LLM calls)     │
  │    │  Interprets intent; takes actions or updates state │
  │    └──────────────────────────► Render (next doc)      │
  │                                                        │
  └────────────────────────────────────────────────────────┘
```

## Step by step

### 1. Render (Typst)

The framework uses Typst — compiled as a Rust library crate, not a CLI — to render the
document. Typst exposes laid-out document frames with precise element positions, which is
how the framework recovers region bounding boxes in PDF-point coordinates. A Typst-based
renderer avoids Chromium and any browser dependency; the renderer is a Rust crate and
nothing more.

The output of this step is a PDF ready to push to the device.

### 2. Embed state — the manifest

Before or during rendering, the framework embeds a **manifest** into the PDF. The manifest
is a structured map of:

- **Regions:** labelled rectangles on each page, each with a name and bounding box in
  PDF-point coordinates.
- **Version marker:** a monotonically increasing version string used to detect stale
  readbacks (ink from an older document should not be interpreted against a newer
  manifest).

The manifest is self-contained in the document. This means the server does not need a
per-session database recording where each user is in the document; the document carries
its own structure. The handler reads the manifest from the PDF alongside the ink, so
state and layout are always in sync.

### 3. Sync to device

The document is pushed to the device via the native `rm-cloud` client (for reMarkable) or the
equivalent device-specific transport. The framework abstracts the transport behind a sync trait, so
the same handler code works across devices. See
[remarkable-pdf-mechanics.md](remarkable-pdf-mechanics.md) for device-level sync rules
(content-only updates, the leading-page-index invariant, annotation preservation).

### 4. User reads and writes

The user reads the document on their pen device and responds with the pen: circling,
checking boxes, underlining, writing in margins, sketching. All annotations are stored
on-device as `.rm` files (one per annotated page), keyed by stable page UUIDs. Nothing
the user writes is lost when the next version of the document is pushed, as long as the
framework's sync invariants are respected.

### 5. Sync back

The device syncs annotations to the cloud. The framework (or the user's device sync)
makes the annotated `.rm` files accessible to the handler.

### 6. Readback (rmfiles)

The `rmfiles` crate — a pure-Rust parser for the `.rm` v6 format — parses the ink files
and produces structured ink strokes (paths, tools, pressures, timestamps). The framework
maps each stroke to a manifest region by containment: a stroke whose centroid or bounding
box falls within a named region is attributed to that region. The version marker is
checked; ink from a stale document version is rejected or flagged.

For cases where raw geometry is insufficient (e.g. reading handwritten text), an LLM can
be called at this step to perform handwriting recognition.

### 7. Process (handler)

A **handler** is user-supplied Rust code that receives the readback result — a structured
description of what the user did, expressed in terms of region names and ink content —
and produces a response: updating state, taking external actions (sending an email,
logging a habit, fetching an article), or preparing the content for the next render
cycle.

The handler is the app-specific logic. The framework supplies everything else.

### 8. Re-render

The handler's output feeds back into step 1. The loop repeats.

---

## Two invariants

**State lives in the document.** The manifest embedded in the PDF is the authoritative
record of what the document looked like when the user wrote on it. The handler receives
the manifest alongside the ink. No out-of-band state store is needed to interpret a
readback; the document is self-describing.

**Secrets are never embedded in documents.** Users may share a document with third
parties (a colleague, a print shop, a friend). API keys, session tokens, personally
identifying information, and anything else sensitive must never appear in the manifest or
elsewhere in the PDF. The manifest carries only structure — region names, bounding boxes,
and version markers — not credentials or private data.

---

## Device-level sync rules

The low-level details of how reMarkable handles an uploaded PDF — the bundle structure,
the content-only update mechanism, the leading-page-index invariant — are documented in
[remarkable-pdf-mechanics.md](remarkable-pdf-mechanics.md). These are the on-device-verified
rules that the framework's sync layer must respect to preserve user ink across document
updates.
