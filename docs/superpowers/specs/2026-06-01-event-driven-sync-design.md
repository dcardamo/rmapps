# Event-driven sync — `rmapps watch`

**Date:** 2026-06-01
**Status:** Design approved in brainstorming; ready for implementation planning.

## Problem

Today `rmapps sync` is a one-shot trigger engine: an external scheduler (cron/systemd
timer) runs it, it evaluates each `[[sync]]` task, runs the due ones, and exits. There is
no resident process and no way to *react* to the reMarkable cloud changing. Two desired
behaviours are impossible:

- When you annotate an article on the tablet, the highlights should flow back to Readwise
  **immediately** — not at the next scheduled reader run.
- When a book gets new highlights, a digest of **that book** should be produced promptly.

We want to react to the tablet syncing with the cloud, **targeted** to specific use cases,
without waiting for a timer — while keeping genuinely time-based jobs (bujo, fetching the
latest Readwise news) on a clock.

## Goals

- A single resident service that reacts to relevant cloud changes via **push** (low latency).
- Reactivity is **opt-in per use case** and **targeted to the changed document** — a book
  getting highlights digests *that book*, not everything.
- Time-based jobs keep running on a schedule, expressible as **wall-clock times** (e.g.
  06:00 and 18:00) or as an interval — unaffected by cloud activity.
- The feature is correct even if the (undocumented) push channel is unavailable.

## Non-goals

- A generic event/action bus or plugin system (YAGNI for two concrete reactions).
- Replacing the per-app generation/state logic each app already has.
- Reacting to every account change. Most changes match no rule and are ignored.

## Decisions (settled during brainstorming)

| Decision | Choice |
| --- | --- |
| Trigger mechanism | **True push** via reMarkable's notification websocket, with a safety-net poll backstop. |
| Runtime model | **One unified daemon** (`rmapps watch`) that runs scheduled jobs *and* reacts to push, replacing the cron/`rmapps sync` invocation. |
| Reaction granularity | **Targeted** to the specific changed document; reactive use cases are opt-in. |
| Config/routing model | **Separate `[[watch]]` rules** from `[[sync]]` scheduled tasks (approach B). |
| Reader behaviour | Fetching news stays **scheduled** (`[[sync]]`); annotation **readback** is the reactive complement (`[[watch]]`). |

## Architecture

`rmapps watch` is a new long-lived command and the one resident service (a systemd unit on
saturn; logs reachable over Tailscale). It owns a single async runtime, one shared
`Cloud`/`Client`, and runs three concurrent subsystems joined by a `tokio::select!` loop:

1. **Scheduler** — fires `[[sync]]` tasks on their intervals/clock-times, in-process. This
   is today's `sync.rs` logic minus the `on-change` trigger (which moves to the reactor).
   Replaces the external cron/`rmapps sync`.
2. **Notification source** — the push channel, behind a trait so the rest of the daemon is
   agnostic to how wakeups arrive:
   ```rust
   trait NotificationSource { async fn next_wakeup(&mut self) -> Wakeup; }
   ```
   - **Real impl:** websocket subscriber to reMarkable's notification service — connects,
     authenticates, yields a `Wakeup` per server message, reconnects with exponential
     backoff on drop.
   - **Fake impl:** emits wakeups on command — used by every reactor test, so the whole
     pipeline is testable with no network.
3. **Reactor** — on each `Wakeup`, runs a *reconcile* (snapshot + diff) and routes the
   results to `[[watch]]` rules.

A `Wakeup` carries **no payload** — it is only a signal. The diff is always the source of
truth for *what* changed, so a websocket message, a safety-net tick, and startup are
interchangeable inputs to the same pipeline.

### Two deliberate built-ins

- **Safety-net reconcile timer.** Even with pure push, websockets drop, miss messages, and
  reconnect. A low-frequency tick (default ~5 min, configurable) feeds the same reconcile
  path as a `Wakeup`. Nearly free for a resident process, and it makes correctness
  independent of websocket reliability. Push still drives the fast path; this is a backstop.
- **Discovery spike for the websocket.** The notification protocol is undocumented, so the
  real `NotificationSource` impl begins with a short spike to capture how the endpoint is
  reached/authenticated (cross-referencing open implementations such as rmfakecloud).
  Because everything sits behind the trait and the safety-net poll exists, **the feature
  works on day one in poll-only mode even before the websocket impl lands** — de-risking the
  one genuinely uncertain piece.

## Config model

`[[sync]]` keeps existing tasks but **loses the `on-change` trigger** — scheduled tasks
become pure timers. Reactive behaviour moves to a new `[[watch]]` section.

```toml
# Scheduled — pure timers, run by the scheduler.
[[sync]]
app = "bujo"
at  = ["06:00", "18:00"]   # wall-clock times each day

[[sync]]
app = "reader"             # pull latest news + deploy, so the device is fresh on wake
at  = ["06:00"]
# `every = "2h"` remains valid as the interval alternative for any task.

# Reactive — run by the reactor when the tablet syncs a matching change.
[[watch]]
path     = "/Books"        # react to doc changes anywhere under this folder
action   = "digest"        # a book got highlights -> digest that book
debounce = "30s"           # coalesce a burst of rapid syncs on the same doc

[[watch]]
path     = "/Read/Library" # rmreader's deployed articles
action   = "readback"      # an article got annotated -> readback that article now
debounce = "30s"
```

### `[[sync]]` scheduler semantics

- `at` is a list of `HH:MM` times in a configured timezone — default the system local zone,
  overridable via a top-level `timezone` (defaults to `[bujo].timezone` if set, else local;
  one obvious source of truth).
- Next fire = soonest listed time after the last run; sleep until then, fire, persist,
  repeat. No busy-waiting.
- `every` and `at` are **mutually exclusive** per task; specifying both is a config error.
- **Catch-up on missed times:** on startup, run any `at` time for today that has passed but
  has not run yet (so a reboot does not cost the morning news pull). Fires **once**, not
  once per missed occurrence across multiple down days.

### `[[watch]]` rule semantics

- `path` — a folder prefix. A changed doc matches when its cloud path is at or under `path`.
  Multiple rules may match one doc; each fires independently.
- `action` — a **closed enum**, initially `"digest"` and `"readback"`. Config validation
  rejects unknown actions; each maps to a real, tested code path. New reactions are new
  enum variants later.
- `debounce` — per-`(rule, doc)` coalescing window; rapid re-syncs of one doc collapse to a
  single action run. Sane default if omitted.

### Validation

Unknown `action`, malformed `path`, bad `debounce`, both `every` and `at` set, or an
unparseable time — all are **hard config errors surfaced at daemon startup**, never swallowed
at runtime.

### `rmapps watch` flags

- `--once` — run one reconcile pass and exit (testing, manual kick).
- `--poll-only` — skip the websocket, use the safety-net timer only (day-one mode; fallback).
- `--poll-interval <dur>` — override the safety-net cadence.

## Data flow (reconcile -> diff -> route -> act)

A `Wakeup` (websocket message, safety-net tick, or startup) feeds one pipeline. Steps are
separated so the interesting logic is pure and testable.

1. **Reconcile.** Fetch a fresh `Snapshot` (`client.snapshot()`). Cheap guard first: if the
   generation has not moved since the persisted baseline, stop — nothing changed (same
   generation check `rm-cloud::sync` already does).
2. **Diff** *(pure)* — compare new snapshot's per-doc hashes against the baseline, producing:
   ```rust
   struct ChangedDoc { id, path, name, prev_hash: Option<String>, new_hash: String, kind: Added | Modified | Removed }
   ```
   This is the per-doc hash comparison `rm-cloud::sync` already does for `changed_keys`,
   lifted into a standalone function over two snapshots.
3. **Filter self-writes** *(loop prevention — critical)* — drop any `ChangedDoc` the daemon
   itself produced: docs carrying the `rmCloudKey` app-key marker, and digest outputs (the
   `exclude_suffixes` digest already uses). Without this, a digest deploy bumps the
   generation, the next reconcile sees "a change under /Books," and the daemon loops
   forever. Explicit and unit-tested with a digest-output fixture.
4. **Route** — for each surviving `ChangedDoc`, match its path against every `[[watch]]`
   rule prefix. Each match produces a `Job { rule, doc }`. No match -> ignored.
5. **Debounce** — key each job by `(rule, doc.id)`. A new job for a waiting key resets its
   timer; when the window elapses, dispatch.
6. **Dispatch** — run the targeted action for `(rule, doc)`. Best-effort: a failure is
   logged and isolated, never killing the daemon or blocking other jobs (same per-task
   isolation as today's `sync`).
7. **Persist baseline** — after a reconcile pass, store the new snapshot (compact `{id ->
   hash}` form) as the baseline so the next diff is against what we have accounted for. A
   job that failed dispatch is recorded for retry (see Error handling) rather than lost.

Steps 2–4 are pure over in-memory snapshots and rules — fully unit-testable with synthetic
data, and integration-testable against the existing fake cloud.

## Targeted actions

Each `action` variant maps to a doc-scoped entrypoint, extracted from today's whole-folder
commands. Shared inner logic, two entry granularities.

- **`readback`** — today `readback::sync_collection` runs over a whole collection folder.
  Add a doc-scoped `readback_one(ChangedDoc)`: fetch that bundle, extract its on-device
  annotations, push to Readwise. The scheduled `reader` task keeps calling the
  collection-wide path; the reactive rule calls the single-doc one.
- **`digest`** — today `generate::run` lists a root and digests docs needing it
  (state-tracked). Add a doc-scoped entrypoint that runs the existing per-doc generate logic
  for one `ChangedDoc`, **reusing the same state file** so reactive and scheduled digests
  never double-process. Output deploys next to the source exactly as now.

Both live behind the existing `BundleFetch` / `Backend` seams, so they are testable with
fakes and no network. Actions are **idempotent** (re-running on the same doc is safe) and
best-effort.

## Error handling

- **Daemon liveness:** no single failure exits the process. Action errors, a malformed doc,
  a Readwise 401 — all logged and isolated, mirroring today's per-task `sync` isolation.
- **Websocket:** drop -> exponential backoff reconnect. The safety-net timer carries
  reconciles meanwhile, so a dead socket degrades to poll latency, never to "stops working."
- **Failed-job retry:** the baseline advances after each reconcile, but a failed dispatch is
  recorded in state (`pending_jobs` keyed by `(rule, doc_id, new_hash)`) and re-attempted on
  the next wakeup. It clears on success or when the doc changes again (superseded). A bounded
  retry count prevents a permanently-failing doc from retrying forever — it is logged and
  dropped after N attempts.
- **Auth/token:** the client already refreshes the session token; the notification socket
  refreshes on reconnect.
- **Corrupt/missing state:** start fresh (current behaviour), then the startup reconcile
  re-derives what is needed.
- **Startup reconcile:** on boot, reconcile against the persisted baseline so changes during
  downtime are processed rather than missed.

## State persistence

Extend the existing atomic `sync-state.json` (temp-write-then-rename, already implemented):

```jsonc
{
  "baseline": { "generation": 1234, "doc_hashes": { "<doc id>": "<hash>" } },
  "last_run": { "<sync task key>": 1717200000 },
  "pending_jobs": [ { "rule": "...", "doc_id": "...", "new_hash": "...", "attempts": 1 } ]
}
```

The compact `{id -> hash}` baseline is enough to diff without storing whole snapshots.

## Module decomposition

- `notify` — `NotificationSource` trait + websocket impl + fake.
- `reconcile` — snapshot diff -> `ChangedDoc` list (pure) + self-write filter (pure).
- `watch` — `[[watch]]` rule matching + debounce + dispatch.
- `actions` — targeted `readback_one` / `digest_one` (extend the reader/digest crates).
- `daemon` (the `watch` command) — owns the runtime and the `select!` loop; wires
  scheduler + notify + reactor against one shared `Cloud`.
- `config` — add `[[watch]]` and `at`/`timezone` parsing + validation; drop `on-change`
  from `[[sync]]`.

## Testing & verification

### Pure unit tests
- Diff: added / modified / removed docs.
- Self-write filter: digest-output and `rmCloudKey` fixtures produce no jobs.
- Path-prefix routing: at/under/sibling paths, multiple matching rules.
- Debounce: coalescing within the window; reset on repeat.
- Scheduler next-fire math: `at` times, timezone, `every`, mutual-exclusion, catch-up.

### Reactor integration (fake cloud)
- Drive the `NotificationSource` fake + the existing fake cloud: commit a doc change, assert
  the right `Job` is produced and the right action invoked (action behind a recording fake).
- Self-write loop: a digest deploy must produce **no** follow-on job.

### Action tests
- `readback_one` / `digest_one` against `BundleFetch` / `Backend` fakes.

### Tier A — automated live e2e (gated; runnable on saturn)
Mirrors `rm-cloud/tests/real_cloud.rs`: gated by `RM_CLOUD_DEVICE_TOKEN` + `#[ignore]`, all
work inside an isolated `rmrs-test/<run-id>` scratch folder, cleaned up on success and left
on failure. The test:
1. Starts the reactor wired to the **real** websocket `NotificationSource` + real `Client`,
   watching `rmrs-test/<run>/Books`.
2. From a second connection, uploads a doc there and then content-changes it (hash bump) —
   indistinguishable to the cloud from the tablet syncing an annotation (same generation
   bump, same doc-hash change, same notification).
3. Asserts the **websocket delivered a wakeup**, the diff produced the expected
   `ChangedDoc`, and the action fired for that doc.
4. Asserts the **self-write filter holds live**: an app-keyed action output into the folder
   produces no follow-on job.
5. Uploads a **captured real annotated bundle fixture** so `readback` / `digest` run on
   realistic `.rm` data (covers the content-extraction path deterministically).

This exercises real auth, real push delivery, the live snapshot/diff, routing, debounce, and
loop-prevention. The only faithful gap is that the change is API-originated rather than
tablet-originated — which the cloud cannot distinguish.

### Tier B — manual real-device check (one-time, with Dan + tablet)
The only thing automation cannot do is make physical ink: highlight a real book on the
Paper Pro, let it sync, and watch the digest appear beside it. Documented exact steps; run
once together to confirm the hardware -> cloud -> reaction loop.

## Open risks

- **Undocumented notification protocol** — mitigated by the discovery spike, the trait
  abstraction, and the poll-only fallback (feature works without it).
- **Baseline size** — mitigated by the compact `{id -> hash}` form.
- **Self-trigger loops** — mitigated by the explicit self-write filter, tested live and in
  the fake cloud.

## Live validation findings (2026-06-01)

Confirmed by running the gated live tests against the real cloud on saturn:

- **Websocket endpoint CONFIRMED.** `wss://internal.cloud.remarkable.com/notifications/ws/json/1`,
  auth `Authorization: Bearer <user-token>`, returns `101 Switching Protocols`. The old
  service-manager discovery host is retired (404 / NXDOMAIN); notifications now live on the
  sync host. Token expiry self-heals (connect path force-refreshes + retries once).
- **Push delivery is keyed by DEVICE IDENTITY, not connection.** Two `Client`s sharing one
  device token do NOT cross-notify: a `broadcast: true` commit from connection B produced no
  frame on subscriber A within 30s. Automating real push delivery therefore requires a
  *second registered device token* (a distinct pairing). The physical tablet is naturally a
  distinct device, so the Tier B manual check validates delivery; an automated equivalent
  needs a second `rmapps auth login` pairing and the test wired to use both tokens
  (subscribe with token A, `commit_broadcast` with token B).
- **`broadcast: false` invariant holds.** All daemon/sync commits use `broadcast: false`, so
  the daemon never self-notifies; only the test opts into `commit_broadcast`.
- **The cloud rate-limits (`429 Too Many Requests`) under burst.** Neither `rm-cloud` nor the
  daemon currently honors `Retry-After`. FOLLOW-UP: add `429` backoff/retry to the rm-cloud
  blob/commit path — relevant for a long-running daemon that reconciles on every push.

The reactor pipeline (diff → route → self-write filter → debounce) is validated against the
fake cloud (`watch::reactor_tests`); the same logic runs unchanged against the real cloud
(the API cannot distinguish a tablet sync from an API commit).
