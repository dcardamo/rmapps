# How PDFs work on reMarkable (what rmbujo relies on)

Distilled, on-device-verified notes about how the reMarkable cloud + tablet treat an
uploaded PDF and its annotations. This is the knowledge rmbujo's "regenerate and
re-sync without losing handwriting" design rests on. Verified on a **Paper Pro Move**
against the **official cloud (v4 sync schema)**, 2026-05-21.

Sources: spikes `docs/superpowers/spikes/2026-05-21-rmapi-lifecycle-spike.md` and
`2026-05-21-pages-links-spike.md`; rmapi source (`ddvk/rmapi`).

## 1. A document is a bundle, not a file

An uploaded PDF on reMarkable is a set of files keyed by a document UUID:

- `<uuid>.pdf` — the PDF you uploaded (the page **backgrounds**).
- `<uuid>/<page-uuid>.rm` — one **handwriting/annotation** file **per annotated page**,
  named by a stable per-page UUID.
- `<uuid>.content` — JSON listing every **visible page in order**; each PDF-backed
  page entry carries the **PDF page index** it shows; user-inserted notebook pages are
  entries here too (with their own page-UUID, no PDF backing).
- plus `.metadata`, `.pagedata`, etc.

## 2. Two independent bindings

- **Ink → page** is keyed by **page-UUID** (the `.rm` filename).
- **Page → background** is keyed by **PDF page index** (recorded in `.content`).

Because they're independent files keyed differently, you can replace the PDF
backgrounds without touching the ink, and vice-versa.

## 3. `rmapi put --content-only` = pure PDF-blob swap

From rmapi's source (`ReplaceDocumentFile` in `api/sync15/apictx.go`): it finds the
document's `.pdf` file entry, uploads the **new PDF blob**, updates that entry's
hash/size, rehashes, and re-uploads the doc index. **It never writes `.content` or any
`.rm`.** Consequences, all verified on-device:

- Annotations and page order (incl. user-inserted pages) are **preserved
  byte-for-byte**; only the PDF backing changes.
- The device **surfaces new trailing PDF pages** (PDF pages beyond what `.content`
  references appear at the end).
- A user-inserted page **cannot be moved by our push** — its position lives in
  `.content`, which content-only doesn't write.

## 4. What's safe vs unsafe to change in the PDF

| Change | Safe under content-only? |
|--------|--------------------------|
| **Append** pages at the END | ✅ verified (5→7→9 pages; ink + inserted page preserved) |
| Edit content of an existing leading page (same index/meaning) | ✅ (ink stays; background refreshes) |
| **Shrink** (remove trailing pages) | likely ✅ (not yet tested) |
| **Insert/reorder in the MIDDLE** of the PDF | ❌ shifts the PDF page indices that `.content` redirects point at → backgrounds land under the wrong pages |

**Invariant rmbujo follows:** never change the *meaning of a leading PDF page index*
the device references; only append/grow/shrink **trailing** pages. (rmbujo keeps the
pages you write on — monthly view, tasks, per-day daily — as a fixed leading section,
and puts volatile calendar pages at the end.)

## 5. User-inserted pages

Inserting a page on the tablet adds a notebook page (its own page-UUID + `.rm`, no PDF
backing) as a `.content` entry at that position. It survives our content-only pushes
(we don't touch `.content`). Note: an inserted page uses the **device's template**, not
your PDF's dot grid — see §8.

## 6. Internal links are real and tappable

fulgur emits `<a href="#id">` (resolved against a block element with `id="id"`) as a
PDF `LinkAnnotation` with an `XyzDestination` (via krilla; see fulgur `src/link.rs`),
and `<a href="https://…">` as a URL action. **Internal links jump correctly on the
Move and survive content-only refresh.** This is what makes month↔day↔agenda↔detail
navigation possible. Cross-*document* links do not work — keep linked pages in one PDF.

## 7. The toolbar covers the top of the page

With the pen toolbar shown, it obscures roughly the **top ~36–40pt (~13–14mm)** of the
page (measured with a ruler page). Start real content below that on every page (cover
pages excepted — full-bleed color behind the toolbar is fine).

## 8. Inserted pages use device templates (need SSH)

A page inserted on the tablet is blank/lined per the device's selected **template**,
not your PDF background. reMarkable has **no** official way to add a custom template
(not via cloud, USB web UI, or rmapi); even GUI tools drive it over the device's SSH.
So shipping a matching dot-grid template requires SSH, and **firmware updates can wipe
custom templates**. Mitigations: pre-allocate dotted pages (`pages_per_day`), or
duplicate an existing dotted page (untested workaround).

## 9. Sync ordering / conflicts

A content-only push only rewrites the `.pdf` blob — a *different file* from your `.rm`
ink and `.content`. rmapi fetches the current cloud hash-tree before modifying (the
"remote tree has changed, refresh" messages) and handles cloud-side generation
conflicts. The discipline: **sync the device → run rmbujo → sync the device**, so the
tablet's edits reach the cloud *before* the push (rmapi can't see un-synced device
changes). No manual download-first is needed on our side.

## 10. rmapi gotchas

- **v4 cloud break:** stock rmapi 0.0.32/0.0.33 fails (HTTP 400 on the `rm-filename`
  header) against the post-2026-05-18 v4 cloud. Fixed by patch (PR #63/#65); rmbujo
  vendors `nix/overlays/rmapi.nix` until nixpkgs ships the fix.
- **`mkdir` is not recursive** — create each ancestor folder (`/base`, then
  `/base/2027`).
- **Token-clobber:** rmapi can zero its own conf on a transient failure or empty-stdin
  auth prompt. Pass `-ni` to non-pairing calls; never feed empty stdin; snapshot the
  conf after pairing.
- **`put` (no flag)** assigns a fresh UUID = new document; use `--content-only`
  (preserve annotations) or `--force` (replace, drops annotations).
- Official-cloud pairing needs a one-time code from
  <https://my.remarkable.com/device/desktop/connect>.

## 11. Determinism

rmbujo's renderer is byte-deterministic; the only non-deterministic input is the
network (ICS fetch), which is cached so regeneration is reproducible. Same input →
identical PDF bytes, so a content-only refresh with unchanged inputs is a no-op on the
device.
