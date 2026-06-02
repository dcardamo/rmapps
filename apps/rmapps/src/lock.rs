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
#[derive(Debug)]
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
            // On Linux EWOULDBLOCK == EAGAIN; match either to stay portable.
            #[allow(unreachable_patterns)]
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
        thread::sleep(Duration::from_millis(50));
        drop(g); // release; waiter should now unblock

        assert_eq!(handle.join().unwrap(), "acquired");
        let _ = std::fs::remove_file(&path);
    }
}
