# Serve loop + run/sync subcommands

**Status:** designed
**Built:** no

## Problem

`publish` (initial push) and `sync_once` (one cycle: pull ink → fold → push/delete) already
exist in `inkapp_core::sync` and are re-exported via the `inkapp` facade. What's missing is a
*loop* that ties them together so the full device round-trip is actually observable: read on
the device, scribble an annotation, watch the framework decode the ink, fold the message
through `update`, and re-publish on the next cycle. Today `apps/reading-queue/src/main.rs`
calls `publish` once and exits — there's no way to see the second half of the loop without
hand-rolling it in every app.

## Goal

Add a reusable, transport-agnostic loop to the framework, plus the CLI surface in the
worked-example app to invoke it.

## Design

### `inkapp_core::sync::serve`

```rust
pub async fn serve<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    set: &mut DocSet,
    transport: &dyn DeviceTransport,
    interval: Duration,
    shutdown: impl Future<Output = ()> + Unpin,
) -> Result<()>
```

- Calls `publish(app, set, transport)` first so the device has documents to read before the
  first pull ever runs.
- Then loops: `tokio::select! { _ = sleep(interval) => {}, _ = &mut shutdown => break }`,
  then `sync_once(app, set, transport)`.
- After each cycle, logs:
  - one summary line: `cycle N: decoded=K ops=push:P delete:D`
  - when non-empty, indented `{:?}` of each decoded msg and each op
  - empty cycles stay on the single summary line
- On `shutdown` resolution, returns `Ok(())` cleanly (no error). One final line: `serve: shutdown`.
- Transport-agnostic by construction: `&dyn DeviceTransport`.
- Shutdown is a parameter (not a hard-coded `ctrl_c` call) so unit tests can drive it with a
  oneshot/Notify without signal plumbing.

Re-exported as `inkapp::serve` (single new line in `crates/inkapp/src/lib.rs`).

### `inkapp::serve` facade

Mirrors `inkapp::publish`/`inkapp::sync_once`: creates an empty `DocSet` and forwards. The
binary builds the `tokio::signal::ctrl_c()` future and passes it as `shutdown`.

### CLI (apps/reading-queue/src/main.rs)

Promote the existing optional `config` field on `Cli` to a `Cmd` enum:

```rust
#[derive(clap::Subcommand)]
enum Cmd {
    Config(cli::ConfigCmd),
    /// Publish, then loop sync_once forever (Ctrl-C to exit).
    Run {
        #[arg(long)]
        interval: Option<u64>,   // seconds; overrides DeviceConfig.sync_interval_secs
    },
    /// One-shot pull + fold + push.
    Sync,
}
```

`Cli.cmd: Option<Cmd>`. No subcommand → today's publish-once behaviour, preserved.
`Run` resolves the interval (CLI flag > config > default), wires `tokio::signal::ctrl_c()`
into `inkapp::serve`. `Sync` calls `inkapp::sync_once` and prints the returned cycle summary.

### DeviceConfig: `sync_interval_secs`

```rust
#[config(default = 30u64)]
pub sync_interval_secs: u64,
```

Lives next to `backend` in `inkapp_core::geometry::DeviceConfig`. Polling cadence is a
deployment concern, not an app concern, so it belongs with the device backend rather than
duplicated into every app's config.

### Tests (TDD, no device, in `crates/inkapp-core/src/sync.rs`)

Extend `FakeTransport`:

- Replace `pulled: Mutex<usize>` with two fields:
  - `pulls_done: Mutex<usize>` (cycle counter)
  - `canned_pulls: Mutex<VecDeque<HashMap<String, Vec<Vec<Stroke>>>>>` (per-call responses)
- `pull` pops the front of `canned_pulls`; empty queue → `HashMap::new()`.
- Add a `deleted: Mutex<Vec<String>>` to observe deletes.
- Helper `FakeTransport::with_pulls(...)` to seed the queue.

Add a tiny test app whose component decodes any ink in its region to an `Archive(key)`
message, and whose `update` removes the key from a `Vec<String>` model. View emits one
`Document::keyed` per remaining key. This is enough to exercise the message → ops → device
delete path without depending on real app components.

Three new tests:

1. **`serve_publishes_before_first_pull`** — drive `serve` with a 1ms interval and an
   already-ready shutdown (`futures::future::ready(())`). Assert the transport saw at least
   one `push` and zero `pull` calls (the publish completes; the loop body never runs).
2. **`sync_once_archives_doc_on_ink`** — queue a single-stroke `HashMap` for `doc-a`,
   call `sync_once`. Assert `cycle.decoded` contains one `Archive("doc-a")`, `cycle.ops`
   contains `DocOp::Delete(DocKey("doc-a".into()))`, and `FakeTransport.deleted` has `"doc-a"`.
3. **`serve_two_cycles_decode_then_quiet`** — queue ink for cycle 1, leave cycle 2 empty.
   Use a `tokio::sync::Notify` so the second cycle triggers shutdown. Assert exactly one
   delete observed, exactly one decoded msg total across the run.

Existing tests (`publish_pushes_every_rendered_doc`,
`sync_once_consults_transport_and_no_ops_without_ink`) remain green — they use
`FakeTransport::default()`, which yields the empty pull queue.

### Logging

Load-bearing — this is what makes the loop observable. Format chosen to be greppable and
short enough to leave running in a terminal:

```
cycle 3: decoded=1 ops=push:0 delete:1
  msg: Archive("doc-a")
  op:  Delete(DocKey("doc-a"))
```

Empty cycles: `cycle 4: decoded=0 ops=push:0 delete:0` (single line, no indent).

### Docs

`docs/appdx.md`: append the `serve` loop to the runtime section as the final step ("the loop
the device round-trip rides on"); mark publish / sync_once / serve as built.

## Non-goals

- No backoff/jitter on the interval — fixed `sleep` is fine for a polling cadence measured in
  tens of seconds.
- No reload of config on the fly — config is read once at process start.
- No structured/JSON logs — `println!` is enough for the worked example. Stdout only.
- No metrics or counters surfaced beyond the per-cycle log line.

## Risks

- **Sibling worktrees**: this branch touches `apps/reading-queue/src/main.rs` (the `Cli` enum)
  and `crates/inkapp/src/lib.rs` (re-exports). Keep additions to single lines so the merge is
  trivial.
- **`Cargo.lock`**: do not stage; dependency bumps go in their own commit.
- **Pre-commit hook** blocks on open native tasks — clear `.tasks.json` before committing.
