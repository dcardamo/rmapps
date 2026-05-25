# The reMarkable Cloud sync protocol (what `rm-cloud` implements)

`crates/rm-cloud` is a pure-Rust client for the **current** reMarkable Cloud. This file is
the on-the-wire reference it implements — the sibling of `remarkable-pdf-mechanics.md`
(which covers on-device PDF/ink behavior). The rules here were established by reading the
Go `rmapi` client (`github.com/ddvk/rmapi`) for protocol knowledge only; **no code was
copied**, and `rm-cloud` deliberately rejects rmapi's filesystem-shaped abstraction in
favor of the content-addressed model below.

## Mental model: a git-style content-addressed store

The cloud is **not** a filesystem. It is:

- **Blobs** addressed by content hash (`sha256`), stored under `/sync/v3/files/<hash>`.
- **Two levels of "index" blob** (plain colon-delimited text): a per-document index lists a
  document's files; the single **root index** lists every document.
- **A root "ref"** holding the current root-index hash and a monotonic **generation**,
  updated by **compare-and-swap** (CAS): a write supplies the generation it read; the server
  rejects it with **HTTP 412** if the generation has since moved.

`rm-cloud` exposes this honestly: an immutable `Snapshot` (`{generation, root_hash, docs}`)
with a pure `diff`, an atomic `commit` (rebase-on-412), path porcelain on top, and a
declarative working-set `sync`.

## Endpoints

| Purpose            | Method | URL                                | Auth          | Body / notes |
|--------------------|--------|------------------------------------|---------------|--------------|
| New device token   | POST   | `{auth}/token/json/2/device/new`   | none          | `{"code","deviceDesc","deviceID"}` → device-token string |
| New user token     | POST   | `{auth}/token/json/2/user/new`     | device bearer | empty body → user-token string |
| Get root ref       | GET    | `{sync}/sync/v4/root`              | user bearer   | → `{"hash","generation","schemaVersion"}`; **404** if never synced |
| Put root ref (CAS) | PUT    | `{sync}/sync/v3/root`             | user bearer   | `{"broadcast","hash","generation"}`, header `rm-filename: roothash` → `{"hash","generation","schemaVersion"}`; **412** if generation stale |
| Get blob           | GET    | `{sync}/sync/v3/files/<hash>`     | user bearer   | header `rm-filename: <logicalName>` → blob bytes (404 if absent) |
| Put blob           | PUT    | `{sync}/sync/v3/files/<hash>`     | user bearer   | header `rm-filename: <logicalName>`, body = bytes |

Default hosts: `auth = https://webapp-prod.cloud.remarkable.engineering`,
`sync = https://internal.cloud.remarkable.com`. `rm-cloud` overrides them with
`RM_CLOUD_HOST` (sets all hosts to one base — used to point at the fake cloud or a proxy).

### Live-cloud requirements (verified against the production cloud)

Three things the production cloud enforces that aren't obvious from the data model — each
was found by running the live test suite, and each has a regression guard:

- **User-token POST needs an explicit `Content-Length`.** `POST /token/json/2/user/new`
  has no body, but the cloud returns **411 Length Required** unless `Content-Length: 0` is
  sent explicitly (reqwest omits it for an empty body).
- **Blob uploads must carry a CRC32C checksum.** `PUT /sync/v3/files/<hash>` is rejected
  with **400 `{"message":"missing checksum"}`** unless it includes
  `x-goog-hash: crc32c=<base64(big-endian CRC32C-Castagnoli of the body)>` (plus a
  `content-type`). Downloads need no checksum.
- **Per-doc indexes come back as schema 4 with a header line.** The device/app writes
  per-doc indexes as schema `4` — `4\n0:<docId>:<count>:<size>\n` then the file lines —
  not the schema `3` (no header) that rmapi's writer emits. The reader must accept both and
  skip the v4 header for per-doc indexes exactly as for the root index.

### Auth

- **Pairing.** POST a one-time 8-char code (from <https://my.remarkable.com/device/desktop/connect>)
  with a fresh `deviceID` UUID → a long-lived **device token**.
- **User token.** POST with the device bearer → a short-lived **user token**, used for all
  sync calls. `rm-cloud` refreshes it lazily and retries once transparently on a 401.

## Hashing rules (lowercase hex; must match byte-for-byte)

- **file hash** = `sha256(file bytes)`.
- **doc hash** = `sha256( concat( hexdecode(fileHash) for files sorted by file id ) )`
  (rmapi's `HashEntries` — a hash of hashes, **not** of the index text).
- **root hash** = `sha256( serialized root-index text bytes )`.

### Blob keying — the non-obvious part

A blob is uploaded under the hash its **parent index lists**, and fetched by that same hash:

- content blobs (`.metadata` / `.content` / `.pdf` / `.rm` / `.pagedata`) → keyed by their
  **file hash** (= `sha256` of content).
- a **doc-index** blob → keyed by the **doc hash** (`HashEntries`), **not** the `sha256` of
  the index text.
- the **root-index** blob → keyed by the **root hash** (= `sha256` of the index text).

So the blob store is a plain `hash → bytes` map; it does **not** verify `key == sha256(bytes)`
(that identity holds only for content + root blobs, not doc-index blobs). `rm-cloud`'s fake
cloud is a plain key→bytes store for exactly this reason.

## Index text formats

Lines are joined with `\n`, each line ending in `\n`. `rm-cloud` always emits schema `4` for
the root index and `3` for per-doc indexes, and accepts both `3` and `4` on read. Entries are
sorted by id before serializing for determinism.

- **Root index (schema 4):**
  ```
  4
  0:.:<docCount>:<totalSize>
  <docHash>:0:<docId>:<numFiles>:<docSize>
  …
  ```
  `docSize` = sum of that document's file sizes; `totalSize` = sum of all `docSize`.

- **Per-doc index (schema 3):**
  ```
  3
  <fileHash>:0:<fileId>:0:<fileSize>
  …
  ```

## Document model

A document id is a UUID; its files are `<id>.metadata`, `<id>.content`, the payload
(`<id>.pdf` / `.epub`), `<id>/<page-uuid>.rm` (one per annotated page), and `<id>.pagedata`.
The doc-index blob's logical name is `<id>.docSchema`; the root-index blob's is
`root.docSchema` (and uses `content-type: text/plain; charset=UTF-8` on upload). A **folder**
is a document whose metadata `type == "CollectionType"` with no payload. This maps 1:1 to
`rm_files::Bundle`, which `rm-cloud` reuses for the on-disk `.rmdoc` representation.

## Write path: atomic commit with rebase-on-412

A mutation (upserts of full document file-sets + removals) commits like this:

1. For each upserted document, compute file hashes, the doc hash, and the doc-index blob;
   **upload all new blobs once** (content-addressed → idempotent across retries).
2. Loop (bounded to 10 attempts):
   a. Fetch the current root ref (its generation).
   b. Apply the mutation to the current doc set → new root-index blob; upload it.
   c. `PUT /root` with the new root hash and the **generation just read**.
   d. On **success**, return. On **412** (a competing writer advanced the generation),
      **rebase**: go back to (a) and re-apply onto the fresh tree. On 401, refresh the user
      token and retry.
3. After the budget is exhausted, return `CommitExhausted`.

Multiple document changes commit as a **single** root PUT → atomic publish. Because
everything is content-addressed and id-keyed, the rebase is a well-defined 3-way merge by
document id.

### Content-only PDF swap (ink preservation)

`put_content_only(id, new_pdf)` replaces only the `<id>.pdf` blob, leaving `.content` and
every `.rm` blob byte-identical. This is the native equivalent of `rmapi put --content-only`
and the reason an updated article keeps the user's on-device annotations — see
`remarkable-pdf-mechanics.md` §3.

## Read path: snapshots and diffing

`snapshot()` does `GET /root` (404 → empty account), fetches the root-index blob, and parses
it into a `Snapshot`. `Snapshot::diff` compares two snapshots' `docId → docHash` maps to
classify added / removed / changed documents — cheap, and the basis for incremental pulls:
"which documents' ink moved" is just "which doc hashes changed," with unchanged blobs skipped
by construction. The declarative `sync(working_set, since)` uses this with a no-op fast path
(unchanged generation → return immediately without fetching blobs).

## Invariants `rm-cloud` keeps

- **Stateless per session.** Snapshots are rebuilt from the cloud each run; there is no local
  session database (matching inkapp's "state lives in the document" invariant). An optional
  in-session blob cache may dedup downloads, but persistence is not required.
- **Secrets never enter a document.** Device/user tokens live only in the client's
  credentials; they never touch any blob, index, or document.
- **App-agnostic.** `rm-cloud` knows blobs and documents — never manifests, regions, Typst,
  or anything inkapp-specific.
