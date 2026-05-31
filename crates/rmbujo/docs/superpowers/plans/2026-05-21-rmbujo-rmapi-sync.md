# rmbujo Phase 2a — reMarkable cloud sync (rmapi) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fill in the `rmapi` deploy backend behind the existing `Deployer` seam so `rmbujo` can upload a year of PDFs to the reMarkable cloud and refresh them non-destructively (preserving on-device handwriting), gated on a device lifecycle spike.

**Architecture:** A vendored Nix overlay provides a working (v4-patched) `rmapi` with `put --content-only`. A manual lifecycle spike proves non-destructive refresh on the real device (go/no-go gate). Then `src/deploy/rmapi.rs` adds an `RmapiRunner` trait (so command sequences are unit-testable without shelling out), an `RmapiDeployer` that builds the `mkdir`/`put`/`put --content-only` sequences, and a `ProcessRmapi` runner that invokes the real binary with a preflight check and a guard against rmapi's token-clobber bug. `get_deployer` learns the `"rmapi"` arm; the wizard gains deploy prompts.

**Tech Stack:** Rust (std `process`/`fs`/`env` only — no new crates), Nix flake + overlay, the external `rmapi` Go binary.

**Spec:** `docs/superpowers/specs/2026-05-21-rmbujo-rmapi-sync-design.md`

---

## File Structure

| File | Responsibility |
|------------------------------------------|---------------------------------------------------------------|
| `nix/overlays/rmapi.nix` (new) | Vendored overlay: patch nixpkgs rmapi with the v4 `rm-filename` fix |
| `flake.nix` (modify) | Wire the overlay; add `rmapi` to the dev shell |
| `docs/superpowers/spikes/2026-05-21-rmapi-lifecycle-spike.md` (new) | Spike findings: working command sequence + conf path + page-stability proof |
| `src/deploy/rmapi.rs` (new) | `RmapiRunner` trait, `RmapiDeployer`, `ProcessRmapi` runner + guard |
| `src/deploy/mod.rs` (modify) | `pub mod rmapi;`; `get_deployer` `"rmapi"` arm |
| `src/config.rs` (modify) | Validate `deploy.backend ∈ {none, rmapi}` |
| `src/wizard.rs` (modify) | Deploy backend + target-folder prompts; thread through `Answers`/`assemble` |
| `tests/deploy.rs` (new) | Command-sequence tests (recording runner) + `ProcessRmapi` shim tests + `get_deployer` routing |
| `tests/config.rs` (modify) | `validate` accepts `rmapi`, rejects bad backend |
| `tests/cli.rs` (modify) | `wizard_assemble` covers deploy fields |

---

## Task 0: Provision rmapi in the flake

**Goal:** A working, v4-patched `rmapi` (with `put --content-only`) is available in the dev shell.

**Files:**
- Create: `nix/overlays/rmapi.nix`
- Modify: `flake.nix`

**Acceptance Criteria:**
- [ ] `nix develop -c sh -c 'command -v rmapi'` prints a path
- [ ] `make test` (i.e. `nix develop -c cargo test`) still passes (overlay doesn't break the build)

**Verify:** `nix develop -c sh -c 'command -v rmapi'` → prints `/nix/store/.../bin/rmapi`

**Steps:**

- [ ] **Step 1: Vendor the overlay**

Create `nix/overlays/rmapi.nix` (provenance: copied from `~/git/dotfiles/overlays/rmapi.nix`; rmbujo is open-source and must not depend on the private dotfiles repo):

```nix
# rmapi overlay — apply ddvk/rmapi's v4 sync-schema fix to nixpkgs' rmapi.
#
# Background: reMarkable rolled out a v4 sync schema on 2026-05-18 that rejects
# rmapi's `rm-filename` HTTP header with HTTP 400 when the value has no file
# extension. nixpkgs' rmapi (0.0.32) sends bare UUIDs / literal "roothash" and
# is rejected on every blob fetch/put. The fix merged upstream 2026-05-20 (PR
# #63) but is not in any release yet, and nixpkgs has not bumped.
#
# PR #65 adds an `ensureExtension()` helper at the BlobStorage boundary that
# defaults missing-extension filenames to `.docSchema`. 0.0.32 already ships
# `put --content-only`, so this overlay yields a fully working rmapi.
#
# REMOVE this overlay once nixpkgs ships rmapi >= the release containing the v4
# fix (>= 0.0.34, whenever ddvk tags it).
self: super: {
  rmapi = super.rmapi.overrideAttrs (old: {
    patches =
      (old.patches or [ ])
      ++ [
        (super.fetchpatch {
          name = "pr-65-ensure-extension-on-rm-filename-header.patch";
          url = "https://github.com/ddvk/rmapi/pull/65.patch";
          hash = "sha256-APwjyV/CV3Xac+DrlrptjYRBo8B1AtjU2ehg4/lJfbg=";
        })
      ];
  });
}
```

- [ ] **Step 2: Wire the overlay and add rmapi to the dev shell**

Edit `flake.nix`. Replace the `let pkgs = import nixpkgs { inherit system; };` binding and add `pkgs.rmapi` to the dev shell `buildInputs`:

```nix
      let
        # flake.nix is at the repo root; the overlay lives at nix/overlays/rmapi.nix.
        overlays = [ (import ./nix/overlays/rmapi.nix) ];
        pkgs = import nixpkgs { inherit system overlays; };
      in {
        devShells.default = pkgs.mkShell {
          # python3: the `stylo` build script (pulled in transitively via
          # fulgur/blitz) generates CSS-property code from .mako.rs templates and
          # shells out to python3. Declared here so the dev shell is self-contained
          # rather than relying on a system Python being on PATH.
          nativeBuildInputs = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.pkg-config pkgs.python3 ];
          # rmapi: reMarkable cloud client, shelled out to by the rmapi deploy
          # backend (v4-patched via overlays/rmapi.nix).
          buildInputs = [ pkgs.libiconv pkgs.fontconfig pkgs.poppler-utils pkgs.dejavu_fonts pkgs.rmapi ];
        };
```

Leave `packages.default` unchanged.

- [ ] **Step 3: Verify rmapi is available**

Run: `nix develop -c sh -c 'command -v rmapi'`
Expected: a `/nix/store/...-rmapi-.../bin/rmapi` path (overlay eval fetches the patch over the network on first build; allow it).

- [ ] **Step 4: Verify the build still passes**

Run: `nix develop -c cargo test`
Expected: all existing tests pass.

- [ ] **Step 5: Commit**

```bash
git add nix/overlays/rmapi.nix flake.nix
git commit -m "Add v4-patched rmapi to the flake dev shell"
```

---

## Task 1: Lifecycle spike — verify non-destructive refresh on the device

**Goal:** Prove on the real Paper Pro Move that `rmapi put --content-only` preserves handwriting and mid-document inserted pages across a regenerate; record the working command sequence and the conf path. **This is a manual, Dan-in-the-loop task and a go/no-go gate** for all productionize tasks.

**Files:**
- Create: `docs/superpowers/spikes/2026-05-21-rmapi-lifecycle-spike.md`

**Acceptance Criteria:**
- [ ] rmapi paired against my.remarkable.com and verified with a real call (`rmapi -ni ls` lists the cloud root without HTTP 400)
- [ ] After `put --content-only` over an annotated doc: handwriting stays on its original pages, the on-device inserted page survives in place, and the background is visibly updated
- [ ] The failure mode (refresh with a different page count) is observed and recorded
- [ ] Spike doc records: the exact working command sequence, the resolved conf path (and which env var controls it), and the page-stability rule
- [ ] **Go/no-go: GO** (if NO-GO, stop and revise the spec before Tasks 2–5)

**Verify:** Manual on-device observation, captured in the committed spike doc.

**Steps:**

- [ ] **Step 1: Pair rmapi (one-time)**

Run `rmapi` interactively (in `nix develop`), paste a one-time code from <https://my.remarkable.com/device/desktop/connect>. Immediately snapshot the conf and confirm the real call works (pairing alone is not proof — the v4 bug let pairing succeed while every call 400'd):

```bash
nix develop -c rmapi -ni ls            # must list the cloud root, no "status 400"
cp ~/.config/rmapi/rmapi.conf /tmp/rmapi-good.conf
```

Record the actual conf path you find (and whether `RMAPI_XDG_HOME`/`XDG_CONFIG_HOME` is in play) — Task 3 needs it.

- [ ] **Step 2: Generate a one-PDF test year and upload**

```bash
nix develop -c cargo run -- new        # year e.g. 2099, daily pages 1, base /tmp/rmbujo-spike
cd /tmp/rmbujo-spike/2099
nix develop -C ~/git/rmbujo -c rmapi -ni mkdir /rmbujo-spike
nix develop -C ~/git/rmbujo -c rmapi -ni put "2099.05 May.pdf" /rmbujo-spike
```

- [ ] **Step 3: Annotate + insert a page on the Move**

On the device: sync, open the doc, write something recognizable on page 1 (the day list), **and insert a blank page in the middle** of the document. Sync again so the cloud has the annotations.

- [ ] **Step 4: Regenerate with a visible change (same page count/order)**

Edit the test year's `rmbujo.toml` or a template to make a *visible* change (e.g. change the theme, or temporarily edit `templates/month_index.html` header text), then regenerate:

```bash
nix develop -C ~/git/rmbujo -c cargo run -- /tmp/rmbujo-spike/2099/rmbujo.toml
```

Confirm the regenerated `2099.05 May.pdf` has the **same page count** as before (`nix develop -c sh -c 'pdfinfo "2099.05 May.pdf"'` — `pdfinfo` is in poppler-utils).

- [ ] **Step 5: Non-destructive refresh and verify**

```bash
nix develop -C ~/git/rmbujo -c rmapi -ni put --content-only "2099.05 May.pdf" /rmbujo-spike
```

On the device: sync and verify (a) your handwriting is still on the pages you wrote it on, (b) the inserted page is still where you put it, (c) the background visibly changed.

- [ ] **Step 6: Characterize the failure mode**

Regenerate with a *different* page count (e.g. set `daily_pages` higher), `put --content-only` again, sync, and record exactly what breaks (mis-mapped backgrounds, error, etc.).

- [ ] **Step 7: Write and commit the spike doc**

Write `docs/superpowers/spikes/2026-05-21-rmapi-lifecycle-spike.md` with sections: **Working command sequence**, **Conf path & env**, **Refresh result (annotations + inserted page)**, **Failure mode (page-count change)**, **Conclusion (GO/NO-GO)**. Then:

```bash
git add docs/superpowers/spikes/2026-05-21-rmapi-lifecycle-spike.md
git commit -m "Spike: verify rmapi put --content-only preserves annotations on the Move"
```

If NO-GO, stop here and revise the spec.

---

## Task 2: RmapiDeployer command sequences

**Goal:** An `RmapiRunner` trait and an `RmapiDeployer` that builds the correct `mkdir`/`put`/`put --content-only` argument sequences, unit-tested with a recording runner (no shelling out).

**Files:**
- Create: `src/deploy/rmapi.rs`
- Modify: `src/deploy/mod.rs` (add `pub mod rmapi;`)
- Test: `tests/deploy.rs`

**Acceptance Criteria:**
- [ ] `deploy()` issues `mkdir <folder>` once, then `put <pdf> <folder>` per PDF
- [ ] `refresh()` issues `put --content-only <pdf> <folder>` per PDF
- [ ] Every rmapi invocation starts with `-ni`
- [ ] `RmapiRunner`, `RmapiDeployer` are public under `rmbujo::deploy::rmapi`

**Verify:** `nix develop -c cargo test --test deploy` → all tests pass

**Steps:**

- [ ] **Step 1: Write the failing tests**

Create `tests/deploy.rs`:

```rust
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use rmbujo::deploy::rmapi::{RmapiDeployer, RmapiRunner};
use rmbujo::deploy::Deployer;

/// Records the args of every rmapi call so tests can assert the sequence.
#[derive(Clone, Default)]
struct Recorder {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
}
impl RmapiRunner for Recorder {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }
}

#[test]
fn deploy_mkdirs_then_puts_each_pdf() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.deploy(&[
        PathBuf::from("/out/2026.05 May.pdf"),
        PathBuf::from("/out/2026 Reference.pdf"),
    ])
    .unwrap();
    let c = rec.calls.borrow();
    assert_eq!(c[0], vec!["-ni", "mkdir", "/2026"]);
    assert_eq!(c[1], vec!["-ni", "put", "/out/2026.05 May.pdf", "/2026"]);
    assert_eq!(c[2], vec!["-ni", "put", "/out/2026 Reference.pdf", "/2026"]);
    assert_eq!(c.len(), 3);
}

#[test]
fn refresh_uses_content_only() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.refresh(&[PathBuf::from("/out/2026.05 May.pdf")]).unwrap();
    let c = rec.calls.borrow();
    assert_eq!(
        c[0],
        vec!["-ni", "put", "--content-only", "/out/2026.05 May.pdf", "/2026"]
    );
    assert_eq!(c.len(), 1);
}

#[test]
fn every_call_is_non_interactive() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.deploy(&[PathBuf::from("/out/a.pdf")]).unwrap();
    d.refresh(&[PathBuf::from("/out/a.pdf")]).unwrap();
    for call in rec.calls.borrow().iter() {
        assert_eq!(call[0], "-ni", "every rmapi call must pass -ni");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test deploy`
Expected: FAIL — `rmbujo::deploy::rmapi` does not exist.

- [ ] **Step 3: Create the module**

Create `src/deploy/rmapi.rs`:

```rust
//! rmapi deploy backend: upload PDFs to the reMarkable cloud and refresh their
//! content non-destructively (preserving on-device handwriting).

use std::path::{Path, PathBuf};

use super::Deployer;

/// Runs a single `rmapi` subcommand. Abstracted so the deploy/refresh command
/// sequences are unit-testable without shelling out to the real binary.
pub trait RmapiRunner {
    /// Run `rmapi <args...>`; `args` never includes the binary name.
    fn run(&self, args: &[&str]) -> anyhow::Result<()>;
}

/// Uploads / refreshes a year of PDFs via an [`RmapiRunner`].
pub struct RmapiDeployer<R: RmapiRunner> {
    target_folder: String,
    runner: R,
}

impl<R: RmapiRunner> RmapiDeployer<R> {
    pub fn new(target_folder: String, runner: R) -> Self {
        Self {
            target_folder,
            runner,
        }
    }

    /// Build the `put` arg vector. `-ni` keeps rmapi non-interactive so it never
    /// blocks on (or clobbers its conf via) the pairing prompt.
    fn put_args<'a>(&'a self, pdf: &'a str, content_only: bool) -> Vec<&'a str> {
        let mut a = vec!["-ni", "put"];
        if content_only {
            a.push("--content-only");
        }
        a.push(pdf);
        a.push(self.target_folder.as_str());
        a
    }
}

impl<R: RmapiRunner> Deployer for RmapiDeployer<R> {
    fn deploy(&self, paths: &[PathBuf]) -> anyhow::Result<()> {
        // mkdir is idempotent: a pre-existing folder makes rmapi error, which we
        // ignore (established rmapi practice). A genuine auth/connectivity
        // failure surfaces on the first `put` below.
        let _ = self.runner.run(&["-ni", "mkdir", self.target_folder.as_str()]);
        for p in paths {
            self.runner.run(&self.put_args(path_str(p)?, false))?;
        }
        Ok(())
    }

    fn refresh(&self, paths: &[PathBuf]) -> anyhow::Result<()> {
        for p in paths {
            self.runner.run(&self.put_args(path_str(p)?, true))?;
        }
        Ok(())
    }
}

fn path_str(p: &Path) -> anyhow::Result<&str> {
    p.to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 path: {}", p.display()))
}
```

- [ ] **Step 4: Declare the submodule**

Edit `src/deploy/mod.rs`: add `pub mod rmapi;` directly below `pub mod local;`:

```rust
pub mod local;
pub mod rmapi;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test --test deploy`
Expected: PASS (3 tests).

- [ ] **Step 6: Format and commit**

```bash
nix develop -c cargo fmt
git add src/deploy/rmapi.rs src/deploy/mod.rs tests/deploy.rs
git commit -m "Add RmapiDeployer command sequences (deploy/refresh) with runner seam"
```

---

## Task 3: ProcessRmapi runner — real binary, preflight, conf guard

**Goal:** A `ProcessRmapi` runner that invokes the real `rmapi` binary non-interactively, refuses to start if rmapi is missing or unpaired, and restores its conf if rmapi's token-clobber bug zeroes it mid-run.

**Files:**
- Modify: `src/deploy/rmapi.rs`
- Test: `tests/deploy.rs`

**Acceptance Criteria:**
- [ ] `ProcessRmapi::with(bin, conf)` errors clearly when `bin` is not an executable file
- [ ] It errors clearly when the conf is missing or has blank tokens
- [ ] A call that fails *and* blanks the conf triggers a restore-from-snapshot + one retry
- [ ] A successful call leaves the conf untouched and logs exactly one invocation

**Verify:** `nix develop -c cargo test --test deploy` → all tests pass

**Steps:**

- [ ] **Step 1: Write the failing tests**

Append to `tests/deploy.rs`:

```rust
use std::os::unix::fs::PermissionsExt;

use rmbujo::deploy::rmapi::ProcessRmapi;

// Unique temp dir without an extra crate (matches the project's test style).
fn tmp_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-deploy-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

const GOOD_CONF: &str = "devicetoken: \"dev-abc\"\nusertoken: \"usr-xyz\"\n";

/// Write an executable `rmapi` shim into `dir`. It logs each call to
/// `dir/calls.log`. If `dir/clobber-trigger` exists, it truncates the conf named
/// in `dir/conf-path`, deletes the trigger, and exits 1 (simulating rmapi's
/// token-clobber-on-failure bug). Otherwise it exits 0.
fn write_shim(dir: &Path) -> PathBuf {
    let shim = dir.join("rmapi");
    std::fs::write(
        &shim,
        "#!/bin/sh\n\
         d=$(dirname \"$0\")\n\
         echo \"$*\" >> \"$d/calls.log\"\n\
         if [ -f \"$d/clobber-trigger\" ]; then\n\
         : > \"$(cat \"$d/conf-path\")\"\n\
         rm -f \"$d/clobber-trigger\"\n\
         exit 1\n\
         fi\n\
         exit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    shim
}

#[test]
fn process_rmapi_rejects_missing_binary() {
    let dir = tmp_dir();
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, GOOD_CONF).unwrap();
    let err = ProcessRmapi::with(dir.join("does-not-exist"), conf).unwrap_err();
    assert!(err.to_string().contains("not"), "got: {err}");
}

#[test]
fn process_rmapi_rejects_unpaired_conf() {
    let dir = tmp_dir();
    let shim = write_shim(&dir);
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, "devicetoken: \"\"\nusertoken: \"\"\n").unwrap();
    let err = ProcessRmapi::with(shim, conf).unwrap_err();
    assert!(err.to_string().contains("pair"), "got: {err}");
}

#[test]
fn process_rmapi_runs_and_logs() {
    let dir = tmp_dir();
    let shim = write_shim(&dir);
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, GOOD_CONF).unwrap();
    let r = ProcessRmapi::with(shim, conf).unwrap();
    r.run(&["-ni", "put", "/out/a.pdf", "/2026"]).unwrap();
    let log = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    assert_eq!(log.trim(), "-ni put /out/a.pdf /2026");
}

#[test]
fn process_rmapi_restores_clobbered_conf_and_retries() {
    let dir = tmp_dir();
    let shim = write_shim(&dir);
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, GOOD_CONF).unwrap();
    // Arm the shim to clobber the conf + fail on its first call.
    std::fs::write(dir.join("conf-path"), conf.to_str().unwrap()).unwrap();
    std::fs::write(dir.join("clobber-trigger"), "").unwrap();

    let r = ProcessRmapi::with(shim, conf.clone()).unwrap();
    r.run(&["-ni", "put", "/out/a.pdf", "/2026"]).unwrap();

    // Conf was restored to the good snapshot, and the call was retried (2 lines).
    assert_eq!(std::fs::read_to_string(&conf).unwrap(), GOOD_CONF);
    let log = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    assert_eq!(log.lines().count(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test deploy`
Expected: FAIL — `ProcessRmapi` does not exist.

- [ ] **Step 3: Implement `ProcessRmapi`**

Append to `src/deploy/rmapi.rs`:

```rust
use std::process::{Command, Stdio};

/// Real runner: invokes the `rmapi` binary. Guards against rmapi's token-clobber
/// bug (it can zero its own conf on a transient failure, bricking later calls) by
/// snapshotting a good conf at construction and restoring it if a call empties it.
pub struct ProcessRmapi {
    bin: PathBuf,
    conf_path: PathBuf,
    snapshot: Vec<u8>,
}

impl ProcessRmapi {
    /// Resolve the default rmapi binary (`rmapi` on PATH) and conf path.
    pub fn new() -> anyhow::Result<Self> {
        Self::with(PathBuf::from("rmapi"), default_conf_path())
    }

    /// Construct with explicit binary + conf paths (used by tests). Verifies both
    /// up front so misconfiguration fails before any upload begins.
    pub fn with(bin: PathBuf, conf_path: PathBuf) -> anyhow::Result<Self> {
        resolve_bin(&bin)?;
        let snapshot = std::fs::read(&conf_path).map_err(|_| {
            anyhow::anyhow!(
                "rmapi is not paired (no conf at {}). Pair once by running `rmapi` \
                 with a code from https://my.remarkable.com/device/desktop/connect",
                conf_path.display()
            )
        })?;
        if is_blank_conf(&snapshot) {
            anyhow::bail!(
                "rmapi conf at {} has blank tokens; re-pair by running `rmapi`",
                conf_path.display()
            );
        }
        Ok(Self {
            bin,
            conf_path,
            snapshot,
        })
    }

    fn attempt(&self, args: &[&str]) -> anyhow::Result<bool> {
        let status = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn conf_blanked(&self) -> bool {
        std::fs::read(&self.conf_path)
            .map(|b| is_blank_conf(&b))
            .unwrap_or(true)
    }
}

impl RmapiRunner for ProcessRmapi {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        if self.attempt(args)? {
            return Ok(());
        }
        // The call failed. If rmapi blanked its own conf, restore the snapshot
        // and retry once before giving up.
        if self.conf_blanked() {
            std::fs::write(&self.conf_path, &self.snapshot)?;
            if self.attempt(args)? {
                return Ok(());
            }
        }
        anyhow::bail!("rmapi {:?} failed", args);
    }
}

fn default_conf_path() -> PathBuf {
    // Mirror rmapi's own resolution: RMAPI_XDG_HOME, then XDG_CONFIG_HOME, then
    // ~/.config. (Confirm against the spike's recorded conf path.)
    if let Ok(p) = std::env::var("RMAPI_XDG_HOME") {
        return PathBuf::from(p).join("rmapi/rmapi.conf");
    }
    if let Ok(p) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(p).join("rmapi/rmapi.conf");
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/rmapi/rmapi.conf")
}

/// Verify the binary is runnable: an explicit path must be an existing file; a
/// bare name must be found on PATH.
fn resolve_bin(bin: &Path) -> anyhow::Result<()> {
    if bin.components().count() > 1 || bin.is_absolute() {
        if bin.is_file() {
            return Ok(());
        }
        anyhow::bail!("`{}` is not an executable file", bin.display());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        if dir.join(bin).is_file() {
            return Ok(());
        }
    }
    anyhow::bail!(
        "`{}` not found on PATH; the flake dev shell provides it (run inside `nix develop`)",
        bin.display()
    )
}

/// A conf is "blank" unless it has a non-empty devicetoken AND usertoken.
/// rmapi's clobber bug writes empty-string values or truncates the file.
fn is_blank_conf(bytes: &[u8]) -> bool {
    let s = String::from_utf8_lossy(bytes);
    let token_ok = |key: &str| {
        s.lines().any(|l| {
            l.trim()
                .strip_prefix(key)
                .map(|rest| {
                    let v = rest.trim_start_matches(':').trim().trim_matches('"');
                    !v.is_empty()
                })
                .unwrap_or(false)
        })
    };
    !(token_ok("devicetoken") && token_ok("usertoken"))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test --test deploy`
Expected: PASS (7 tests total).

- [ ] **Step 5: Format and commit**

```bash
nix develop -c cargo fmt
git add src/deploy/rmapi.rs tests/deploy.rs
git commit -m "Add ProcessRmapi runner: preflight + token-clobber conf guard"
```

---

## Task 4: Wire get_deployer and validate the backend

**Goal:** `get_deployer` returns an `RmapiDeployer<ProcessRmapi>` for `backend = "rmapi"`, and `Config::validate` rejects unknown backends.

**Files:**
- Modify: `src/deploy/mod.rs`
- Modify: `src/config.rs`
- Test: `tests/deploy.rs`, `tests/config.rs`

**Acceptance Criteria:**
- [ ] `get_deployer` with `backend = "none"` → `Ok` (LocalDeployer); unknown backend → `Err`
- [ ] `get_deployer` with `backend = "rmapi"` and empty `target_folder` → `Err` mentioning `target_folder` (checked before any rmapi preflight, so the test needs no rmapi)
- [ ] `Config::validate` accepts `deploy.backend` of `none`/`rmapi`, rejects others

**Verify:** `nix develop -c cargo test` → all tests pass

**Steps:**

- [ ] **Step 1: Write the failing tests**

Append to `tests/deploy.rs`:

```rust
use rmbujo::config::Config;
use rmbujo::deploy::get_deployer;

#[test]
fn get_deployer_routes_backends() {
    // none → ok
    assert!(get_deployer(&Config::new(2026)).is_ok());
    // unknown → err
    let bogus = Config {
        deploy: rmbujo::config::DeployConfig {
            backend: "bogus".into(),
            target_folder: "/2026".into(),
        },
        ..Config::new(2026)
    };
    assert!(get_deployer(&bogus).is_err());
    // rmapi with empty target_folder → err before any rmapi preflight
    let no_folder = Config {
        deploy: rmbujo::config::DeployConfig {
            backend: "rmapi".into(),
            target_folder: "  ".into(),
        },
        ..Config::new(2026)
    };
    let err = get_deployer(&no_folder).unwrap_err();
    assert!(err.to_string().contains("target_folder"), "got: {err}");
}
```

Append to `tests/config.rs` (inside the existing file):

```rust
#[test]
fn validate_deploy_backend() {
    assert!(Config {
        deploy: config::DeployConfig {
            backend: "rmapi".into(),
            target_folder: "/2026".into(),
        },
        ..Config::new(2026)
    }
    .validate()
    .is_ok());
    assert!(Config {
        deploy: config::DeployConfig {
            backend: "ftp".into(),
            target_folder: "/2026".into(),
        },
        ..Config::new(2026)
    }
    .validate()
    .is_err());
}
```

Note: `tests/config.rs` imports `rmbujo::config::{self, Config, IcsFeed}` already, so `config::DeployConfig` resolves.

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --test deploy --test config`
Expected: FAIL — `get_deployer` bails on `"rmapi"`; `validate` doesn't check the backend yet.

- [ ] **Step 3: Extend `get_deployer`**

Edit `src/deploy/mod.rs` `get_deployer`:

```rust
pub fn get_deployer(config: &Config) -> anyhow::Result<Box<dyn Deployer>> {
    match config.deploy.backend.as_str() {
        "none" => Ok(Box::new(local::LocalDeployer)),
        "rmapi" => {
            // Validate config before touching rmapi, so a misconfig fails the same
            // way regardless of whether rmapi is installed/paired.
            if config.deploy.target_folder.trim().is_empty() {
                anyhow::bail!("deploy.target_folder is required for the rmapi backend");
            }
            let runner = rmapi::ProcessRmapi::new()?;
            Ok(Box::new(rmapi::RmapiDeployer::new(
                config.deploy.target_folder.clone(),
                runner,
            )))
        }
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
}
```

- [ ] **Step 4: Add backend validation to `Config::validate`**

In `src/config.rs`, inside `validate`, add after the `week_start` match (before the `spacing_mm` check):

```rust
        match self.deploy.backend.as_str() {
            "none" | "rmapi" => {}
            other => anyhow::bail!("deploy.backend must be 'none' or 'rmapi', got {other:?}"),
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test`
Expected: PASS (full suite green).

- [ ] **Step 6: Format and commit**

```bash
nix develop -c cargo fmt
git add src/deploy/mod.rs src/config.rs tests/deploy.rs tests/config.rs
git commit -m "Route the rmapi deploy backend in get_deployer; validate deploy.backend"
```

---

## Task 5: Wizard prompts for deploy backend + target folder

**Goal:** `rmbujo new` asks for the deploy backend and reMarkable folder, writing them into `rmbujo.toml`.

**Files:**
- Modify: `src/wizard.rs`
- Test: `tests/cli.rs`

**Acceptance Criteria:**
- [ ] `Answers` carries `deploy_backend` and `target_folder`; `assemble` writes them into `Config.deploy`
- [ ] `run_wizard` prompts for both (defaults: `none`, `/<year>`)
- [ ] `wizard_assemble` test asserts the deploy fields round-trip into the config

**Verify:** `nix develop -c cargo test --test cli` → all tests pass

**Steps:**

- [ ] **Step 1: Update the failing test**

Edit `tests/cli.rs` `wizard_assemble` to pass the new fields and assert them. Replace the `assemble(Answers { ... })` call and add assertions:

```rust
#[test]
fn wizard_assemble() {
    let base = tmp_dir();
    let (config, out_dir, config_path) = assemble(Answers {
        year: 2026,
        base: base.to_string_lossy().into_owned(),
        device: "paper-pro-move".into(),
        week_start: "sun".into(),
        daily_pages: 3,
        collection_pages: 2,
        spacing_mm: 4.5,
        theme: "library".into(),
        deploy_backend: "rmapi".into(),
        target_folder: "/2026".into(),
    });
    assert_eq!(config.year, 2026);
    assert_eq!(config.daily_pages, 3);
    assert_eq!(config.spacing_mm, 4.5);
    assert_eq!(config.deploy.backend, "rmapi");
    assert_eq!(config.deploy.target_folder, "/2026");
    assert_eq!(out_dir, base.join("2026"));
    assert_eq!(config_path, base.join("2026").join("rmbujo.toml"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `nix develop -c cargo test --test cli`
Expected: FAIL — `Answers` has no `deploy_backend`/`target_folder` fields.

- [ ] **Step 3: Add fields to `Answers` and use them in `assemble`**

Edit `src/wizard.rs`. Add to `struct Answers`:

```rust
    pub spacing_mm: f32,
    pub theme: String,
    pub deploy_backend: String,
    pub target_folder: String,
}
```

In `assemble`, replace the hardcoded `DeployConfig`:

```rust
        theme: a.theme,
        ics: Vec::new(),
        deploy: DeployConfig {
            backend: a.deploy_backend,
            target_folder: a.target_folder,
        },
    };
```

- [ ] **Step 4: Add the prompts in `run_wizard`**

In `src/wizard.rs` `run_wizard`, after the `theme` prompt and before the `assemble` call, add:

```rust
    let deploy_backend: String = Input::new()
        .with_prompt("Deploy backend (none|rmapi)")
        .default("none".into())
        .interact_text()?;
    let target_folder: String = Input::new()
        .with_prompt("reMarkable folder")
        .default(format!("/{year}"))
        .interact_text()?;
```

Then add both to the `Answers { ... }` literal passed to `assemble`:

```rust
        theme,
        deploy_backend,
        target_folder,
    });
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test`
Expected: PASS (full suite green).

- [ ] **Step 6: Format and commit**

```bash
nix develop -c cargo fmt
git add src/wizard.rs tests/cli.rs
git commit -m "Wizard: prompt for deploy backend and reMarkable folder"
```

---

## Task 6: Documentation + final green

**Goal:** README reflects that rmapi sync is implemented (no longer "Phase 2"); the full suite, clippy, and fmt pass.

**Files:**
- Modify: `README.md`

**Acceptance Criteria:**
- [ ] README documents `deploy.backend = "rmapi"` + `target_folder`, the one-time pairing step, and that regeneration re-syncs via `put --content-only`
- [ ] `make test`, `make clippy`, `make fmt-check` all pass

**Verify:** `make test && make clippy && make fmt-check` → all succeed

**Steps:**

- [ ] **Step 1: Update the README**

In `README.md`, change the trailing "Phase 2" note. Replace the line:

```
ICS calendar feeds (incl. holidays) and reMarkable cloud sync (via rmapi) are Phase 2;
see `docs/superpowers/specs/2026-05-20-rmbujo-design.md`.
```

with:

```
## reMarkable cloud sync (rmapi)

Set `deploy.backend = "rmapi"` and `deploy.target_folder = "/2026"` in `rmbujo.toml`
(the `new` wizard prompts for both). Pair once: run `rmapi` and paste a code from
<https://my.remarkable.com/device/desktop/connect>. Then:

- `rmbujo new` uploads the year's PDFs to the cloud folder.
- `rmbujo path/to/rmbujo.toml` regenerates and re-syncs with `rmapi put --content-only`,
  which replaces each PDF's background **without touching your handwriting**.

ICS calendar feeds (incl. holidays) are the next phase; see
`docs/superpowers/specs/2026-05-20-rmbujo-design.md`.
```

- [ ] **Step 2: Run the full gate**

Run: `make test && make clippy && make fmt-check`
Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "Document rmapi cloud sync in the README"
```

---

## Self-Review notes

- **Spec coverage:** §Provisioning → Task 0; §Lifecycle spike → Task 1; §`deploy/rmapi.rs` (`deploy`/`refresh`, runner seam) → Tasks 2–3; guard rails (`-ni`, conf guard, clear errors) → Tasks 2–3; `get_deployer` "rmapi" arm + validation → Task 4; §Wizard → Task 5; §Testing (recording runner + shim + preflight) → Tasks 2–4; README/docs → Task 6.
- **Determinism:** deploy/refresh are side effects; no PDF-generation or golden tests are touched, so byte-determinism is unaffected.
- **No new crates:** runner/guard use only std (`process`, `fs`, `env`, `os::unix`), matching the project's dependency-light test style (homemade `tmp_dir`).
- **Type consistency:** `RmapiRunner::run(&[&str])`, `RmapiDeployer::new(String, R)`, `ProcessRmapi::{new, with}`, `is_blank_conf`, `get_deployer` arms, `Answers.{deploy_backend,target_folder}` are used identically across tasks.
- **Gate:** Tasks 4 and 5 (wiring + user exposure) and Task 1 are the parts that depend on the spike's GO; Tasks 2–3 are pure tested logic and only depend on the module existing.
