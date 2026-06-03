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

## Deploy / restart on saturn (don't fight the auto-rebuild)

On saturn the `rmapps-watch` daemon runs the compiled binary, and systemd
automation rebuilds + restarts it for you: a `remarkable-update` timer (~5 min)
rebuilds the release binary whenever **local `HEAD` advances** (it builds the
local checkout, not just pushed commits), and a path unit bounces the daemon when
the binary changes.

So after you `git commit` rmapps code, **do not** manually
`sudo systemctl restart rmapps-watch` to pick it up — committing already bumped
`HEAD`, so the updater will rebuild and the daemon will restart on its own within
a few minutes. A manual restart in that window **races the automation**: the
rebuild/restart units stop and replace your instance, so `systemctl status` shows
`code=killed, signal=TERM` and looks like a crash — it isn't. To pick up a code
change immediately, force the rebuild instead: `sudo systemctl start
remarkable-update` (synchronous; the daemon restarts when it finishes).

A manual restart **is** the right move only for **config-only** changes — the
config (`~/.config/rmapps/config.toml`) is home-manager–owned and lives outside
this repo, so it doesn't bump `HEAD`; deploy it with `make update` in
`~/git/dotfiles`, then `sudo systemctl restart rmapps-watch`.
