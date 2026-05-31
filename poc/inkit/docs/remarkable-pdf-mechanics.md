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

## 3. Content-only update = pure PDF-blob swap

A content-only update finds the document's `.pdf` file entry, uploads the **new PDF
blob**, updates that entry's hash/size, rehashes, and re-uploads the doc index. **It
never writes `.content` or any `.rm`.** inkapp does this natively via
`rm_cloud::Client::put_content_only` (the same mechanic the now-retired `rmapi
put --content-only` implemented in its `ReplaceDocumentFile`). Consequences, all
verified on-device:

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

## 8. Inserted pages use device templates — so match a built-in one

A page inserted on the tablet is blank/lined per the device's selected **template**,
not your PDF background. reMarkable has **no** official way to add a *custom* template
(not via cloud, USB web UI, or rmapi; even GUI tools drive it over SSH), and firmware
updates can wipe sideloaded ones.

**rmbujo's approach: match a built-in template instead of sideloading.** We set our
dot grid to the pitch of reMarkable's built-in **"Dots Small"** template, so a user who
inserts a page and picks "Dots Small" gets a page that matches our generated pages —
no sideloading, no SSH for users. **Measured pitch (from an exported template page):
"Dots Small" = 4.756 mm** (42.5 reMarkable units × 0.31718 pt/unit = 13.48 pt, uniform
x/y; "Lines Small" = 5.82 mm, for a future lined option). That becomes rmbujo's default
dot spacing. (`pages_per_day` still lets you pre-allocate dotted pages so inserting is
rarely needed.)

## 9. Sync ordering / conflicts

A content-only push only rewrites the `.pdf` blob — a *different file* from your `.rm`
ink and `.content`. The `rm-cloud` client fetches the current root snapshot before
committing and resolves cloud-side generation conflicts by rebasing on a 412
(compare-and-swap on the root generation). The discipline: **sync the device → run the
app → sync the device**, so the tablet's edits reach the cloud *before* the push (the
cloud can't see un-synced device changes). No manual download-first is needed on our side.

## 10. Transport notes (native `rm-cloud` client)

inkapp speaks the reMarkable cloud protocol directly via the pure-Rust `rm-cloud`
crate — there is no `rmapi` CLI dependency. The protocol facts that shaped its design:

- **`mkdir` is not recursive at the protocol level** — each folder is its own document
  with a `parent` id, so a path is created one level at a time. `rm_cloud::Client::mkdir_p`
  walks the path and creates each missing ancestor.
- **A new document gets a fresh UUID.** `Client::put` of a freshly built `DocFiles`
  (e.g. `DocFiles::new_pdf`) creates a new document; the ink-preserving update is
  `put_content_only` (swap the `.pdf` blob only — see §3).
- **Auth** is by device/user token (`RM_CLOUD_DEVICE_TOKEN` / `RM_CLOUD_USER_TOKEN`);
  the device token is minted once via the one-time code at
  <https://my.remarkable.com/device/desktop/connect>. The user token is refreshed
  automatically from the device token. Tokens live in the environment, not a CLI conf
  file, so there is no token-clobber failure mode.

## 11. Determinism

rmbujo's renderer is byte-deterministic; the only non-deterministic input is the
network (ICS fetch), which is cached so regeneration is reproducible. Same input →
identical PDF bytes, so a content-only refresh with unchanged inputs is a no-op on the
device.
