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
