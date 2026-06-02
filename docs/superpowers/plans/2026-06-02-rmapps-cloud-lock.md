# rmapps single-instance cloud lock — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serialize all cloud-mutating `rmapps` operations through a single advisory file lock so the systemd daemon and worktree builds never run cloud ops in parallel.

**Architecture:** A new `lock` module wraps `~/.local/state/rmapps/cloud.lock` with `flock` (via `rustix`). The resident `watch` daemon acquires the lock **per task** in **blocking** mode (tasks queue, never dropped) and holds nothing while idle; all interactive one-shot subcommands acquire **once** in `main.rs` in **fail-fast** mode, reporting the current holder. flock auto-releases on process exit, so crashes never leave a stale lock.

**Tech Stack:** Rust, `rustix::fs::flock`, `anyhow`, `dirs`.

**User Verification:** NO — no user verification required (infrastructure locking layer; spec requests no human sign-off).

---

### Task 1: `lock` module with flock-based cloud lock

**Goal:** A self-contained `lock` module providing `Wait`, `CloudLock`, and `acquire`/`acquire_at`, with tests.

**Files:**
- Modify: `apps/rmapps/Cargo.toml` (add `rustix` dep)
- Create: `apps/rmapps/src/lock.rs`
- Modify: `apps/rmapps/src/main.rs` (register `mod lock;`)

**Acceptance Criteria:**
- [ ] `acquire_at(path, op, Wait::Fail)` returns `Err` while another guard holds the lock, with a message containing `pid` and `op=<holder op>`.
- [ ] Dropping a `CloudLock` releases the lock (a subsequent `Wait::Fail` acquire succeeds).
- [ ] `acquire_at(path, op, Wait::Block)` blocks while held and returns once the holder's guard is dropped.

**Verify:** `nix develop --command cargo test -p rmapps lock` → all lock tests PASS

**Steps:**

- [ ] **Step 1: Add the `rustix` dependency**

In `apps/rmapps/Cargo.toml`, under `[dependencies]` (after the `dirs = "5"` line), add:

```toml
rustix = { version = "1", features = ["fs"] }
```

- [ ] **Step 2: Register the module**

In `apps/rmapps/src/main.rs`, add `mod lock;` to the module list (keep alphabetical-ish ordering, e.g. after `mod ls;`):

```rust
mod ls;
mod lock;
mod push;
```

- [ ] **Step 3: Write `lock.rs` with the implementation and tests**

Create `apps/rmapps/src/lock.rs`:

```rust
//! Single-instance advisory lock around cloud-mutating operations.
//!
//! Every `rmapps` process that mutates the reMarkable cloud coordinates through
//! one advisory flock at `~/.local/state/rmapps/cloud.lock`. The path is fixed
//! (not derived from the binary/checkout location) so the systemd daemon and any
//! worktree build serialise against each other.
//!
//! flock is released automatically when the holding process exits (the kernel
//! closes the fd), so a crash never leaves a stale lock. The resident `watch`
//! daemon acquires per task in `Wait::Block` mode and holds nothing while idle;
//! interactive one-shot commands acquire once in `Wait::Fail` mode and report
//! the current holder.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rustix::fs::{flock, FlockOperation};

/// Contention behaviour when the lock is already held.
#[derive(Clone, Copy, Debug)]
pub enum Wait {
    /// Block until the lock can be acquired (the resident daemon: tasks queue).
    Block,
    /// Fail immediately with a holder-identifying error (interactive commands).
    Fail,
}

/// RAII guard. Dropping it closes the fd, which releases the flock.
pub struct CloudLock {
    _file: File,
}

/// `~/.local/state/rmapps/cloud.lock` (same base as sync-state.json).
fn lock_path() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("rmapps").join("cloud.lock")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Acquire the cloud lock at the default path. See [`acquire_at`].
pub fn acquire(op: &str, wait: Wait) -> Result<CloudLock> {
    acquire_at(&lock_path(), op, wait)
}

/// Acquire the cloud lock at `path` (parameterised for tests).
pub fn acquire_at(path: &Path, op: &str, wait: Wait) -> Result<CloudLock> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating lock dir {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .with_context(|| format!("opening lock file {}", path.display()))?;

    match wait {
        Wait::Block => {
            flock(&file, FlockOperation::LockExclusive)
                .context("acquiring cloud lock (blocking)")?;
        }
        Wait::Fail => match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(rustix::io::Errno::WOULDBLOCK) | Err(rustix::io::Errno::AGAIN) => {
                let holder = read_holder(&mut file);
                anyhow::bail!(
                    "another rmapps cloud op in progress{holder} — stop it \
                     (e.g. `systemctl stop rmapps-watch`) or wait"
                );
            }
            Err(e) => return Err(anyhow::Error::new(e).context("acquiring cloud lock")),
        },
    }

    // We now hold the lock: record holder metadata for any future failing acquirer.
    let _ = file.set_len(0);
    let _ = file.seek(SeekFrom::Start(0));
    let _ = write!(
        file,
        "pid={} op={} since={}",
        std::process::id(),
        op,
        now_unix()
    );
    let _ = file.flush();

    Ok(CloudLock { _file: file })
}

/// Read the holder line and format it for an error message, e.g.
/// " (pid 1234, op=sync, started 14s ago)". Returns "" if unreadable.
fn read_holder(file: &mut File) -> String {
    let mut buf = String::new();
    if file.seek(SeekFrom::Start(0)).is_err() {
        return String::new();
    }
    if file.read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
        return String::new();
    }
    let mut pid = "?";
    let mut op = "?";
    let mut since: Option<u64> = None;
    for tok in buf.split_whitespace() {
        if let Some(v) = tok.strip_prefix("pid=") {
            pid = v;
        } else if let Some(v) = tok.strip_prefix("op=") {
            op = v;
        } else if let Some(v) = tok.strip_prefix("since=") {
            since = v.parse().ok();
        }
    }
    let ago = since
        .map(|s| format!(", started {}s ago", now_unix().saturating_sub(s)))
        .unwrap_or_default();
    format!(" (pid {pid}, op={op}{ago})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn tmp_lock(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rmapps-test-{}-{}.lock", name, std::process::id()))
    }

    #[test]
    fn fail_errors_while_held() {
        let path = tmp_lock("fail");
        let _g = acquire_at(&path, "held", Wait::Block).unwrap();
        let err = acquire_at(&path, "second", Wait::Fail).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("pid"), "message should name holder pid: {msg}");
        assert!(msg.contains("op=held"), "message should name holder op: {msg}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn release_on_drop() {
        let path = tmp_lock("drop");
        {
            let _g = acquire_at(&path, "first", Wait::Block).unwrap();
        } // guard dropped here
        // Lock is now free: a fail-fast acquire must succeed.
        let _g2 = acquire_at(&path, "second", Wait::Fail).expect("lock should be free after drop");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn block_waits_then_succeeds() {
        let path = tmp_lock("block");
        let g = acquire_at(&path, "holder", Wait::Block).unwrap();

        let (tx, rx) = mpsc::channel();
        let p2 = path.clone();
        let handle = thread::spawn(move || {
            tx.send(()).unwrap(); // signal: about to block on acquire
            let _g2 = acquire_at(&p2, "waiter", Wait::Block).unwrap();
            "acquired"
        });

        rx.recv().unwrap();
        // Give the waiter time to actually enter the blocking flock call, so this
        // genuinely exercises the blocking path (not a crutch for correctness:
        // the waiter acquires regardless of ordering).
        thread::sleep(Duration::from_millis(50));
        drop(g); // release; waiter should now unblock

        assert_eq!(handle.join().unwrap(), "acquired");
        let _ = std::fs::remove_file(&path);
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nix develop --command cargo test -p rmapps lock`
Expected: `fail_errors_while_held`, `release_on_drop`, `block_waits_then_succeeds` all PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/rmapps/Cargo.toml apps/rmapps/Cargo.lock apps/rmapps/src/lock.rs apps/rmapps/src/main.rs
git commit -m "feat(rmapps): add single-instance cloud lock module"
```

(Note: `Cargo.lock` is at the workspace root — `git add Cargo.lock` if that is where it lives.)

---

### Task 2: Wire the lock into the one-shot commands and the watch daemon

**Goal:** All cloud-mutating subcommands acquire the lock at exactly one layer: one-shots fail-fast in `main.rs`; `watch` blocks per task.

**Files:**
- Modify: `apps/rmapps/src/main.rs` (one-shot dispatch arms)
- Modify: `apps/rmapps/src/watch/mod.rs` (scheduled-task and reactive-job call sites)

**Acceptance Criteria:**
- [ ] `main.rs` acquires `Wait::Fail` for `Sync`, `Reader`, `Bujo`, `Digest`, `Push`, `Rm`; leaves `Auth`, `Ls`, `Watch` unlocked at the dispatch layer.
- [ ] `watch`'s scheduled-task path (`run_due_scheduled`) acquires `Wait::Block` per task around `crate::sync::run_task`, releasing between tasks.
- [ ] `watch`'s reactive-job path (`run_one_job`) acquires `Wait::Block` around `crate::watch::actions::run_job`.
- [ ] Workspace builds cleanly.

**Verify:** `nix develop --command cargo build -p rmapps` → builds with no errors; `nix develop --command cargo test -p rmapps` → existing tests still PASS

**Steps:**

- [ ] **Step 1: Wrap the one-shot dispatch arms in `main.rs`**

Replace the mutating arms of the `match cli.command` block (in `fn main`) so each acquires a fail-fast lock before running. The lock guard is bound to `_lock` so it lives for the duration of the handler and releases when `main` returns. Leave `Auth`, `Ls`, and `Watch` exactly as they are.

```rust
        Command::Bujo(args) => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("bujo", lock::Wait::Fail)?;
            bujo::run(args, &cfg)
        }
        Command::Reader => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("reader", lock::Wait::Fail)?;
            reader::run(&cfg)
        }
        Command::Digest(args) => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("digest", lock::Wait::Fail)?;
            digest::run(args, &cfg)
        }
        Command::Sync => {
            let cfg = config::load(cfg_path)?;
            let _lock = lock::acquire("sync", lock::Wait::Fail)?;
            sync::run(&cfg)
        }
        Command::Push(args) => {
            let _lock = lock::acquire("push", lock::Wait::Fail)?;
            push::run(args)
        }
        Command::Rm(args) => {
            let _lock = lock::acquire("rm", lock::Wait::Fail)?;
            rm::run(args)
        }
```

Leave these unchanged:

```rust
        Command::Auth(args) => auth::run(args),
        Command::Watch(args) => {
            let cfg = config::load(cfg_path)?;
            watch::run(args, &cfg)
        }
        Command::Ls(args) => ls::run(args),
```

- [ ] **Step 2: Acquire a blocking per-task lock in `run_due_scheduled`**

In `apps/rmapps/src/watch/mod.rs`, inside `run_due_scheduled`, the `if due { ... }` block currently reads:

```rust
            println!("[rmapps] watch: running scheduled {key}");
            match crate::sync::run_task(task, &key, cfg) {
                Ok(()) => {
                    state.last_run.insert(key, now_secs());
                }
                Err(e) => eprintln!("[rmapps] watch: scheduled {key} failed: {e:#}"),
            }
```

Replace it with a version that takes the cloud lock (blocking) for just this task. The guard drops at the end of the `if due` block, releasing the lock before the next loop iteration:

```rust
            println!("[rmapps] watch: running scheduled {key}");
            let _lock = match crate::lock::acquire(&key, crate::lock::Wait::Block) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[rmapps] watch: cloud lock error for {key}: {e:#}");
                    continue;
                }
            };
            match crate::sync::run_task(task, &key, cfg) {
                Ok(()) => {
                    state.last_run.insert(key, now_secs());
                }
                Err(e) => eprintln!("[rmapps] watch: scheduled {key} failed: {e:#}"),
            }
```

- [ ] **Step 3: Acquire a blocking lock in `run_one_job`**

In `apps/rmapps/src/watch/mod.rs`, `run_one_job` currently starts:

```rust
fn run_one_job(cloud: &Cloud, cfg: &Config, state: &mut state::WatchState, job: &Job) {
    match crate::watch::actions::run_job(cloud, cfg, job) {
```

Insert a blocking lock acquire before the `match` (the guard drops at function end):

```rust
fn run_one_job(cloud: &Cloud, cfg: &Config, state: &mut state::WatchState, job: &Job) {
    let _lock = match crate::lock::acquire("watch-job", crate::lock::Wait::Block) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[rmapps] watch: cloud lock error for job: {e:#}");
            return;
        }
    };
    match crate::watch::actions::run_job(cloud, cfg, job) {
```

- [ ] **Step 4: Build and run the test suite**

Run: `nix develop --command cargo build -p rmapps`
Expected: builds with no errors.

Run: `nix develop --command cargo test -p rmapps`
Expected: all tests PASS (lock tests + pre-existing watch/sync tests).

- [ ] **Step 5: Commit**

```bash
git add apps/rmapps/src/main.rs apps/rmapps/src/watch/mod.rs
git commit -m "feat(rmapps): serialize cloud ops via the cloud lock (watch blocks, one-shots fail fast)"
```

---

### Task 3: Document the contention failure path in a repo-root CLAUDE.md

**Goal:** A repo-root `CLAUDE.md` telling future Claude sessions how to react when an interactive `rmapps` command fails because the lock is held.

**Files:**
- Create: `CLAUDE.md`

**Acceptance Criteria:**
- [ ] `CLAUDE.md` exists at repo root and explains the "another rmapps cloud op in progress" failure: identify the holder pid from the message; if it is the systemd `rmapps-watch` daemon (saturn), stop it, re-run, and restart or leave it stopped while iterating; do not blind-retry.

**Verify:** `test -f CLAUDE.md && grep -q "rmapps-watch" CLAUDE.md && echo OK` → prints `OK`

**Steps:**

- [ ] **Step 1: Create `CLAUDE.md`**

Create `CLAUDE.md` at the repo root:

```markdown
# rmapps

Unified CLI + resident daemon for the reMarkable toolset (auth, bujo, reader,
digest, sync, watch, push) over the native `rm-cloud` client.

## Single-instance cloud lock

All cloud-mutating operations serialize through one advisory lock at
`~/.local/state/rmapps/cloud.lock` (see `apps/rmapps/src/lock.rs`). The resident
`watch` daemon acquires it per task in blocking mode (tasks queue, never
dropped); interactive one-shot commands (`sync`, `reader`, `bujo`, `digest`,
`push`, `rm`) acquire it fail-fast.

### Handling "another rmapps cloud op in progress"

An interactive command failing with:

> another rmapps cloud op in progress (pid 1234, op=…, started Ns ago) — stop it …

means another `rmapps` process currently holds the lock — almost always the
systemd `rmapps-watch` daemon (saturn only) running a task. To resolve:

1. Read the holder **pid** and **op** from the message.
2. If it is the `rmapps-watch` daemon, stop it, then re-run your command:
   ```bash
   sudo systemctl stop rmapps-watch     # on NixOS use /run/wrappers/bin/sudo if PATH resolves the wrong sudo
   # … run your rmapps command …
   sudo systemctl start rmapps-watch    # restart when done, or leave stopped while iterating
   ```
3. If the holder is another manual run (not the daemon), wait for it to finish.

Do **not** blind-retry in a loop — identify the holder and act on it. The daemon
exists only on saturn; on other hosts a held lock is always another manual run.
```

- [ ] **Step 2: Verify and commit**

Run: `test -f CLAUDE.md && grep -q "rmapps-watch" CLAUDE.md && echo OK`
Expected: `OK`

```bash
git add CLAUDE.md
git commit -m "docs: document rmapps cloud-lock contention handling in CLAUDE.md"
```
