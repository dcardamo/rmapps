# Sync timing instrumentation (`tracing`)

**Date:** 2026-06-02
**Status:** Approved (design)

## Problem

`rmapps sync` is slow. The hypothesis is that wall-clock time goes into
*building content* (PDF/Typst rendering, image fetching) rather than the cloud
upload itself — but we have no measurements. Before optimizing anything we need
per-stage timing so we optimize the stage that is actually slow rather than
guessing.

This change adds measurement only. Optimization is a separate follow-up once we
have real numbers from a live sync.

## Goal

Instrument the sync pipeline with `tracing` spans so a single run prints a
hierarchical per-stage timing breakdown, and make the timing output toggleable
(off by default).

Non-goals: changing any sync behavior, parallelizing work, tuning the rate
governor, or removing the existing ad-hoc `eprintln!` timing lines (they can
stay or be migrated opportunistically, but wholesale cleanup is out of scope).

## Approach

Adopt `tracing` + `tracing-subscriber` (chosen over a bespoke timer so we get
real spans, env-filtering, and a standard, future-proof foundation).

- **Library crates emit spans only.** `rmreader`, `rmbujo`, `rmdigest`, and
  `rm-cloud` gain a `tracing` dependency and span instrumentation. They never
  install a subscriber.
- **The `rmapps` binary owns subscriber setup.** `main.rs` installs exactly one
  global subscriber, gated by the toggle. This avoids library crates fighting
  over global subscriber state and keeps them reusable.

### Dependencies

Workspace additions:

- `tracing` — added to `apps/rmapps`, `crates/rmreader`, `crates/rmbujo`,
  `crates/rmdigest`, `crates/rm-cloud`. Near-zero cost when no subscriber
  collects spans.
- `tracing-subscriber` with features `fmt` and `env-filter` — added to
  `apps/rmapps` only.

Pin via workspace dependency table in the root `Cargo.toml` so versions stay
aligned.

### Instrumentation points (span tree)

Spans nest to produce the hierarchy. Use `#[tracing::instrument]` on functions,
plus manual `span!` where one function runs several distinct phases.

- `sync.run` (root span for a sync invocation)
  - `task{name="reader"}`
    - `reader.readback`
    - `reader.fetch_docs`
    - `reader.image_fetch`
    - `reader.typst_render`
    - `reader.upload`
  - `task{name="bujo"}`
    - `bujo.ics_fetch`
    - `bujo.generate` (covers per-PDF generation)
    - `bujo.upload`
  - `task{name="digest"}`
    - `digest.list`
    - `digest.bundle_fetch`
    - `digest.ingest`
    - `digest.extract`
    - `digest.render`
    - `digest.upload`
- `rm-cloud` low-level spans, so we can separate the rate-governed upload path
  from content building:
  - `cloud.put_blob`
  - `cloud.commit`

Span names use dotted prefixes for readability under the `fmt` formatter. Field
values (e.g. task name, doc counts) are attached where cheap and useful.

### Toggle and output

**Off by default.** Timing output is enabled by either:

- the global CLI flag `--timings`, or
- the environment variable `RMAPPS_TIMINGS=1` (truthy values: `1`, `true`,
  `yes`, case-insensitive).

If both are present, the flag wins. The env var exists so the systemd
`rmapps-watch` daemon can enable timing without passing flags.

When enabled, the binary installs a `tracing-subscriber` `fmt` layer configured
with `FmtSpan::CLOSE`, so each span logs its `busy`/`idle` duration when it
closes, indented by nesting. Example shape:

```
reader.typst_render  close  time.busy=18.1s
reader.image_fetch   close  time.busy=12.4s
task{name=reader}    close  time.busy=42.1s
sync.run             close  time.busy=48.6s
```

When disabled, **no subscriber is installed**, so spans compile to near-nothing
and produce zero log noise.

`env-filter` is wired so power users can additionally use `RUST_LOG=...` for
finer control, but the flag / env var is the documented, supported path. When
`--timings`/`RMAPPS_TIMINGS` is on and no `RUST_LOG` is set, default the filter
to a level that shows the instrumentation spans.

### CLI wiring

`--timings` is a global flag parsed before subcommand dispatch in `main.rs`, so
it applies to any subcommand (not just `sync`). The toggle resolution (flag OR
env var) happens once at startup, before the subscriber decision.

## Testing

- **Toggle/init unit test:** the subscriber-init helper respects the toggle —
  off → no layer installed; on → layer installed — and is safe to call once
  (no double-init panic).
- **Span emission test:** attach a capturing test subscriber (e.g. via a
  custom collecting layer or `tracing-test`) and assert that running the sync
  entry path emits at least `sync.run` and one `task{...}` span, so the
  instrumentation cannot silently rot.
- **Regression:** all existing tests continue to pass; instrumentation is
  purely additive and changes no sync behavior.

## Follow-up (out of scope here)

After landing this, run a real `rmapps sync` with `--timings`, capture the
breakdown, and open a separate optimization effort targeting whichever
stage(s) dominate (candidates from code review: serial Typst rendering, serial
Bujo PDF generation, serial digest bundle downloads, rate-governed serial
uploads).
