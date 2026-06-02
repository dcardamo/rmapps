# rmapps single-instance cloud lock — design

**Date:** 2026-06-02
**Status:** approved, ready for implementation

## Problem

`rmapps` runs in two ways that hit the **same** reMarkable cloud and output dirs:

- The systemd `rmapps-watch` daemon on saturn, running `rmapps watch` from
  `~/git/rmapps` (origin/main only).
- Manual / iteration runs (`sync`, `reader`, `bujo`, `digest`, `push`, `rm`,
  or a dev `watch`) built in a worktree's `target/`, while iterating with Claude.

Two cloud-mutating operations running at once can race or corrupt cloud state
(e.g. `replace()` sweeping same-named docs). We must guarantee that **no two
cloud-mutating operations run in parallel**.

## Requirements

1. Never two cloud-mutating operations at once.
2. The long-running daemon must **not** hold the lock while idle — only while
   actually executing a cloud-mutating task.
3. On contention: the **daemon waits** (scheduled tasks must never be silently
   dropped); **interactive one-shot commands fail fast** with a message naming
   the current holder, so the operator can stop the daemon or wait.
4. No priority machinery and no role flag / env var — behavior is keyed purely
   on the subcommand. The operator stops the systemd unit manually if it is in
   the way.

Out of scope: read-only commands (`ls`, `auth` status) do not participate.
Preemption is at a task boundary — a half-finished cloud upload is never killed.

## Design

### Lock file

- Path: `~/.local/state/rmapps/cloud.lock`, resolved via `dirs::state_dir()`
  with the same fallback chain already used for `sync-state.json`. Parent dir is
  created if missing.
- Fixed, location-independent path → the systemd checkout and any worktree
  binary coordinate through the same lock. It is **not** derived from the repo
  or binary location.
- Mechanism: `rustix::fs::flock` (already in the dependency tree).
  `LOCK_EX` for blocking acquire, `LOCK_EX | LOCK_NB` for fail-fast.
- Crash safety: flock is released automatically when the holding process exits
  (the kernel closes the fd), so a crash never leaves a stale lock.

### New module `apps/rmapps/src/lock.rs`

```rust
pub enum Wait { Block, Fail }

/// RAII guard. Dropping it closes the fd and releases the flock.
pub struct CloudLock { /* open fd */ }

/// Acquire the cloud lock. `op` is a short label (e.g. "sync", "push") written
/// into the lock file for diagnostics.
pub fn acquire(op: &str, wait: Wait) -> anyhow::Result<CloudLock>;
```

Behavior:

- Open (create if absent) the lock file.
- `Block`: `flock(LOCK_EX)` — wait until acquired.
- `Fail`: `flock(LOCK_EX | LOCK_NB)`. On `EWOULDBLOCK`, read the existing holder
  line and return an error:
  `another rmapps cloud op in progress (pid <pid>, op=<op>, started <N>s ago) — stop it or wait`
- On success, truncate the file and write a single holder line:
  `pid=<pid> op=<op> since=<unix_ts>`.

Holder metadata is always live when read: it is only read after a *failed*
try-lock, which means a process currently holds the lock and therefore wrote
that line on its own (successful) acquire. Stale metadata from a crashed holder
is never read, because the next acquirer would succeed and overwrite it. flock
is advisory and does not block reads, so the failing acquirer can read the line
while another process holds the lock.

### Acquisition points — exactly one per process

The lock is acquired at exactly one layer per process. This is essential:
`sync` internally calls `bujo::run` / `reader::run` / `digest::run`, and flock
locks are per open-file-description — a second independent `open` + `flock` from
the same process conflicts with the first. So the handlers must stay
lock-agnostic and the lock is taken above them.

| Subcommand                                         | Lock behavior |
|----------------------------------------------------|---------------|
| `watch`                                            | per-task **Block**, acquired inside the loop around each task/job execution, released between tasks |
| `sync`, `reader`, `bujo`, `digest`, `push`, `rm`   | once in `main.rs` dispatch, **Fail**, held for the whole command |
| `auth`, `ls`                                       | none |

- **One-shot commands**: `main.rs` acquires `Fail` *before* calling the handler.
  On error it prints the message and exits non-zero. Handlers do not lock, so
  `sync` invoking them internally never re-locks.
- **`watch`**: does **not** lock at `main` (it is mostly idle and must hold
  nothing while idle). It wraps, with `Block`:
  - the per-task `crate::sync::run_task` call in `run_due_scheduled`
    (`watch/mod.rs:208`), acquired per task so the lock frees between tasks; and
  - the reactive `crate::watch::actions::run_job` call (`watch/mod.rs:247`).

### Why this satisfies the requirements

- Single `LOCK_EX` → no two cloud-mutating ops overlap (req 1).
- `watch` acquires per-task and releases between tasks; idle daemon holds
  nothing (req 2).
- `watch` uses `Block` (tasks queue, never dropped); one-shots use `Fail` and
  report the holder (req 3).
- Behavior is keyed only on subcommand; no flag or env var (req 4).

## Testing

Integration test against a temp lock path (flock is per-OFD, so two threads each
doing their own `open` + `flock` suffice; no subprocess needed):

1. `Fail` returns an error while the lock is held by another guard.
2. `Block` waits and then succeeds once the holder's guard is dropped.
3. The `Fail` error message contains the holder's pid and op label.
4. Dropping a `CloudLock` releases the lock (a subsequent `Fail` succeeds).

## Documentation

Create a repo-root `CLAUDE.md` documenting the fail path: an interactive
`rmapps` command failing with "another rmapps cloud op in progress" means the
systemd `rmapps-watch` daemon (saturn only) is mid cloud-task. Resolution:
identify the holder pid from the message; if it is the daemon, stop it
(`sudo systemctl stop rmapps-watch`, using the `/run/wrappers/bin/sudo` PATH fix
on NixOS), re-run the command, and restart it (`systemctl start rmapps-watch`)
or leave it stopped while iterating. Do not blind-retry.
