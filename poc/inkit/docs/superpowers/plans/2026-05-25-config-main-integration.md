# Config ↔ main Integration Plan (land end-user configuration on main, folding `deploy.toml` in)

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers-extended-cc:subagent-driven-development or executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Land the `config` branch's end-user configuration system (`inkapp-config`) on top of `main`, making `config.toml` the single config mechanism and folding main's `deploy.toml` deployment config into it — while adopting main's `DeviceTransport`/`sync` architecture and preserving main's `inkapp-content`/image pipeline.

**Architecture:** The config work already exists, reviewed and green, on branch `config` (HEAD `c10e2f1`). main diverged 55 commits with a *better* transport architecture (`inkapp_core::sync` engine + object-safe `DeviceTransport` seam + `rm-device::RmTransport`, apps' `serve.rs` deleted) and a small, **unwired** `deploy.toml` mechanism (`crates/inkapp/src/deploy.rs`). This plan does NOT merge the branches; it creates a fresh branch off main and **ports** the conflict-free config crates verbatim, then re-applies the app/connector migration onto main's structure with specific adaptations, folding deploy into a framework `[device]` config section.

**Tech stack:** Rust 2021, serde, toml/toml_edit, inventory, clap, reqwest, syn/quote. Tests via `nix develop -c cargo …`.

**Source branches:** integrate onto `main` (`/home/dan/git/inkapp`, HEAD `e8fccfe`); port from `config` (`/home/dan/.paseo/worktrees/2ymhz306/just-frog`, HEAD `c10e2f1`). Read config-branch files via `git show config:<path>` or from that worktree.

**Key decisions (settled with Dan):**
1. `inkapp-config` (`config.toml`) is the single config mechanism; main's `deploy.toml` folds in.
2. Deploy config = framework **`[device].backend`** + per-app **`device_folder`** (already in the app config section).
3. Apps resolve config, build the transport via a facade `resolve` helper, and **pass it** to `publish`/`sync_once`; the apps' deploy path (currently render-only on main) gets **finished**.
4. **Fresh integration branch off main, re-apply** (no tangled merge).
5. Renumber the config spec/plan from **#12 → #13** (main owns #12).
6. Adopt main's transport seam; **drop** the config branch's obsolete `serve.rs` folder-threading.

---

## Pre-flight facts about main (from reconnaissance — verify before relying)
- No `inkapp-config`, `config.toml`, `ConfigStore`, `#[derive(Config)]`, `SecretRef`/`ConnectorRef`, or `config` CLI exist on main → the two config crates port in with zero conflict.
- `inkapp-core::secrets::SecretStore` unchanged (XDG, 3 scopes) → `SecretRef` resolves into it; **secrets never enter `config.toml`**.
- `inkapp-remarkable` was **renamed to `rm-device`** (commit `934ec6e`). Transport is `inkapp_core::sync::{publish,sync_once}` over `trait DeviceTransport` (`inkapp-core/src/sync.rs:17-25`); `rm-device::RmTransport` is the impl; the facade `crates/inkapp/src/deploy.rs` resolves it from `deploy.toml`.
- `deploy.rs` `DeployConfig{backend,folder}` is parsed from `$INKAPP_DEPLOY_CONFIG` (no XDG); apps' `main()` only `render` (never call publish/sync_once). `Error::Config(String)` is in `inkapp-core/src/error.rs`.
- main's `App` builder likely has **no `.page()`** setter (render hardwires `PageGeom::default()`) — Task 2 verifies and restores it.
- main's `ReaderConfig` is code-built (no token field, no derive); `Readwise::live(secrets, cache_dir, ReaderConfig)` still exists and pulls the token from `SecretStore["readwise-reader"]`. main's reading-queue renders `inkapp_content::Article`s with `image_urls()` — **must preserve**.
- main `.gitignore` ignores `/apps/*/deploy.toml`.

---

### Task 1: Integration branch + port the two config crates (clean)

**Goal:** A new branch off main carrying `inkapp-config` + `inkapp-config-derive` verbatim, building and testing in isolation.

**Files:**
- Create worktree/branch off `main`.
- Copy: `crates/inkapp-config/**`, `crates/inkapp-config-derive/**` from `config` (`c10e2f1`).
- Modify: root `Cargo.toml` (`[workspace] members`).

**Acceptance:**
- [ ] New branch `config-integration` (or similar) off `main`'s HEAD.
- [ ] Both crates present, added to workspace members.
- [ ] `nix develop -c cargo test -p inkapp-config --features cli` passes (same tests as on the config branch).

**Steps:**
- [ ] **Create the worktree off main.** From the main repo: `git -C /home/dan/git/inkapp worktree add -b config-integration <path> main`. Work there for all subsequent tasks. (Or use the using-git-worktrees skill.)
- [ ] **Copy the crates** from the config branch verbatim:
  `git -C <integration-worktree> checkout config -- crates/inkapp-config crates/inkapp-config-derive` (this stages them; they're new paths on main so no conflict). If `git checkout <branch> -- <path>` isn't convenient across worktrees, copy the directories from `/home/dan/.paseo/worktrees/2ymhz306/just-frog/crates/inkapp-config{,−derive}`.
- [ ] **Add to workspace members** in root `Cargo.toml`, after `crates/inkapp`:
  ```toml
      "crates/inkapp-config",
      "crates/inkapp-config-derive",
  ```
- [ ] **Verify:** `nix develop -c cargo test -p inkapp-config --features cli` → all pass; `nix develop -c cargo build -p inkapp-config-derive`.
- [ ] **Commit:** `git add crates/inkapp-config crates/inkapp-config-derive Cargo.toml && git commit -m "inkapp-config[-derive]: port end-user config crates onto main (Spec #13 task 1)"` (Cargo.lock unstaged).

---

### Task 2: PageConfig in core + restore `.page()` builder setter

**Goal:** `[page]` config section in `inkapp-core` → `PageGeom`, and an `App` builder `.page()` setter to receive it.

**Files:** Modify `crates/inkapp-core/Cargo.toml` (add `inkapp-config` dep), `crates/inkapp-core/src/geometry.rs` (port `PageConfig` + `From`), `crates/inkapp-core/src/runtime.rs` (ensure `.page()` setter on the builder).

**Acceptance:**
- [ ] `PageConfig` (kind=page, namespace=framework) ports in; defaults 420/560/16 == `PageGeom::default()`.
- [ ] The `App` builder exposes `.page(PageGeom) -> Self`; render uses it (default unchanged when not called).
- [ ] `nix develop -c cargo test -p inkapp-core geometry` passes.

**Steps:**
- [ ] Add `inkapp-config = { path = "../inkapp-config" }` to `inkapp-core/Cargo.toml`.
- [ ] Port `PageConfig` + `impl From<PageConfig> for PageGeom` + the two geometry tests from `git show config:crates/inkapp-core/src/geometry.rs` (the block added on the config branch).
- [ ] **Verify/restore `.page()`:** read main's `crates/inkapp-core/src/runtime.rs` builder. If a `.page(geom)` setter is absent (main likely removed it), add it to the final builder stage and thread `geom: PageGeom` into `App::new`/render exactly as the config branch did (`git show config:crates/inkapp-core/src/runtime.rs` for the pattern). Keep `PageGeom::default()` as the default. If main's `App::new` signature changed (commit `caaf1d1` touched it), adapt — match main's current `App::new`.
- [ ] **Verify:** `nix develop -c cargo test -p inkapp-core` (geometry + runtime).
- [ ] **Commit:** "inkapp-core: PageConfig + builder .page() setter (Spec #13 task 2)".

---

### Task 3: ReaderConfig-as-Config + `Readwise::from_config` (onto main's Readwise)

**Goal:** Re-apply the Readwise config changes onto main's evolved connector (which feeds `inkapp-content` Article rendering).

**Files:** Modify `crates/inkapp-readwise-reader/{Cargo.toml,src/lib.rs}`; port `tests/config_ctor.rs`.

**Acceptance:**
- [ ] `ReaderConfig` derives `Config` (kind=readwise) + `token: SecretRef`; existing defaults preserved.
- [ ] `Readwise::from_config(cfg, &SecretStore, cache_dir)` replaces `live(...)`; empty/absent token → `ConnectorError::Auth`; **30s HTTP timeout** retained.
- [ ] main's article-rendering path (Article body, `image_urls`) still compiles/works.
- [ ] `nix develop -c cargo test -p inkapp-readwise-reader` passes.

**Steps:**
- [ ] Add `inkapp-config` dep.
- [ ] Diff main's `ReaderConfig`/`live` (`crates/inkapp-readwise-reader/src/lib.rs:236,548`) against the config-branch version (`git show config:crates/inkapp-readwise-reader/src/lib.rs`). Apply: derive `Config` + `#[serde(default)]` + `token: SecretRef`; remove manual `Default`; replace `live` with `from_config` (token from `cfg.token.name()`, empty-token guard); keep the retrying client's **30s timeout** (config branch commit `0c3e5c5`).
- [ ] Update all `live` callers on main (grep `Readwise::live` / `.live(` — tests, examples) to `from_config`.
- [ ] **Preserve** anything main added to `Readwise`/`Article` integration; this task only swaps the config/constructor surface, not the article pipeline.
- [ ] Port `tests/config_ctor.rs` (the Auth-variant + empty-token tests, using `.err()` not `unwrap_err` since `Readwise` isn't `Debug`).
- [ ] **Verify:** `nix develop -c cargo test -p inkapp-readwise-reader`.
- [ ] **Commit:** "inkapp-readwise-reader: ReaderConfig as Config + from_config (Spec #13 task 3)".

---

### Task 4: ICS config + HTTP refresh (verify against main's ICS)

**Goal:** Port `IcsConfig` + `Source::{Url,Inline}` + HTTP `refresh` + `from_config` (+30s timeout).

**Files:** Modify `crates/inkapp-ics/{Cargo.toml,src/lib.rs}`; port `tests/live.rs`.

**Acceptance:**
- [ ] `IcsConfig` (kind=ics) with `url`; `Source::{Url,Inline}`; `from_fixture`→Inline, `from_config`→Url; `refresh` fetches Url via reqwest (30s timeout), error preserves warm cache; empty url errors.
- [ ] `nix develop -c cargo test -p inkapp-ics` passes; `nix develop -c cargo build -p agenda` builds.

**Steps:**
- [ ] Diff main's `crates/inkapp-ics/src/lib.rs` against `git show config:crates/inkapp-ics/src/lib.rs`. If main's ICS is unchanged from the shared base (likely), apply the config-branch version wholesale: deps (`inkapp-config`, `reqwest` rustls-tls), `IcsConfig`, `Source`, threaded struct, HTTP `refresh` with 30s timeout, tests, `tests/live.rs`.
- [ ] **Verify:** `nix develop -c cargo test -p inkapp-ics`.
- [ ] **Commit:** "inkapp-ics: IcsConfig + Url/Inline HTTP refresh (Spec #13 task 4)".

---

### Task 5: LocalCal config

**Goal:** Port `LocalCalConfig` + fallible `from_config` (errors on empty `store_path`).

**Files:** Modify `crates/inkapp-localcal/{Cargo.toml,src/lib.rs}`.

**Acceptance:**
- [ ] `LocalCalConfig` (kind=localcal) with `store_path`; `from_config(&cfg) -> Result<Self, ConfigError>` errors (`Missing`) on empty path.
- [ ] `nix develop -c cargo test -p inkapp-localcal` passes.

**Steps:**
- [ ] Diff main vs `git show config:crates/inkapp-localcal/src/lib.rs`; apply `inkapp-config` dep, `LocalCalConfig`, the fallible `from_config` (empty-path → `ConfigError::Missing`), and tests.
- [ ] **Verify:** `nix develop -c cargo test -p inkapp-localcal`.
- [ ] **Commit:** "inkapp-localcal: LocalCalConfig + from_config (Spec #13 task 5)".

---

### Task 6: Facade re-exports

**Goal:** Re-export the config surface through `inkapp`.

**Files:** Modify `crates/inkapp/{Cargo.toml,src/lib.rs}`.

**Acceptance:**
- [ ] `inkapp` deps `inkapp-config` (cli feature); re-exports `ConfigStore`, `select_instance`, `Config`, `Namespace`, `ConfigError`, `SecretRef`, `ConnectorRef`, `cli`.
- [ ] `nix develop -c cargo build -p inkapp` builds (no name clash with main's existing facade exports — check `Config`/`Namespace`).

**Steps:**
- [ ] Add the dep + the two `pub use inkapp_config::…` lines (from `git show config:crates/inkapp/src/lib.rs`) to main's facade lib.rs (which also re-exports `deploy`/`sync` — keep those).
- [ ] **Verify build + clippy.**
- [ ] **Commit:** "inkapp facade: re-export config surface (Spec #13 task 6)".

---

### Task 7: Fold `deploy.toml` into config — `[device]` section + transport-param publish/sync_once

**Goal:** Replace main's `DeployConfig`/`INKAPP_DEPLOY_CONFIG` with a config-driven `[device].backend`; make `publish`/`sync_once` take an explicit `&dyn DeviceTransport`; expose a facade `resolve_transport(backend, folder)`.

**Files:** Modify `crates/inkapp/src/deploy.rs`, `crates/inkapp/src/lib.rs`; add a `DeviceConfig` section (in `inkapp-core/src/geometry.rs` alongside PageConfig, or a small `inkapp-core/src/device_config.rs` — framework namespace, kind="device").

**Acceptance:**
- [ ] A framework config section `DeviceConfig { backend: String (default "remarkable") }` deriving `Config` (namespace=framework, kind=device).
- [ ] `inkapp::resolve_transport(backend: &str, folder: String) -> Result<Box<dyn DeviceTransport>>` keeps the `match backend { "remarkable" => RmTransport::new(folder), … }` in the facade (so `inkapp-config` never depends on `rm-device`).
- [ ] `publish(&mut app, transport: &dyn DeviceTransport)` and `sync_once(&mut app, transport: &dyn DeviceTransport)` (no internal env/config read).
- [ ] `DeployConfig` + `from_env`/`INKAPP_DEPLOY_CONFIG` removed; the `Error::Config` variant may stay (reused by `resolve_transport` for unknown backend).
- [ ] `nix develop -c cargo build -p inkapp` + `cargo test -p inkapp-core` (sync engine tests still pass).

**Steps:**
- [ ] Add `DeviceConfig` (framework section) — derive `Config`, `backend` default `"remarkable"`.
- [ ] Rewrite `deploy.rs`: drop `DeployConfig`/`from_env`/`from_path`; keep `resolve` but rename/signature `pub fn resolve_transport(backend: &str, folder: String) -> Result<Box<dyn DeviceTransport>>`; change `publish`/`sync_once` to accept `transport: &dyn DeviceTransport` and drop the internal `DocSet::default()` only if the app now owns the set (keep main's set-ownership choice — verify how main's wrappers manage `DocSet`; preserve that behavior, just inject the transport).
- [ ] Update facade re-exports for the new signatures.
- [ ] **Verify** build + the existing `inkapp-core::sync` tests + `rm-device` transport tests still pass.
- [ ] **Commit:** "inkapp: fold deploy into [device] config; publish/sync_once take a transport (Spec #13 task 7)".

---

### Task 8: Migrate `reading-queue` (config-driven + finish deploy wiring)

**Goal:** reading-queue resolves its instance from `config.toml`, wires the Readwise connector + page geometry from config, resolves `[device].backend` + its `device_folder`, builds the transport, and **calls `publish`/`sync_once`** (finishing main's render-only path); mounts the `config` CLI. Preserve Article rendering + `image_urls`.

**Files:** Modify `apps/reading-queue/{Cargo.toml,src/main.rs,src/lib.rs}`.

**Acceptance:**
- [ ] `AppConfig` (kind=reading-queue, namespace=app) with `device_folder` + `readwise: ConnectorRef`; `Connectors::from_config` (port, mapping connector failures to `ConfigError::Connector`).
- [ ] `main()`: clap with `--instance` + `config` subcommand; resolve `AppConfig`+`PageConfig`+`DeviceConfig`; SecretStore user_key; build connectors from config (persistent XDG cache dir); `.page(page.into())`; build transport via `resolve_transport(device.backend, app_cfg.device_folder)`; render then `publish`/`sync_once`. Article rendering + `image_urls` preserved.
- [ ] `nix develop -c cargo test -p reading-queue` passes.

**Steps:**
- [ ] Add `inkapp-config` + `clap` deps.
- [ ] Port `AppConfig` + `Connectors::from_config` from `git show config:apps/reading-queue/src/lib.rs`, but **rebase onto main's lib.rs** which renders `inkapp_content::Article` and delegates `image_urls` — keep all of main's view/component code; add only the config struct + constructor. Keep main's `Connectors::{persisted,…}` constructors used by tests.
- [ ] Rewrite `main.rs` combining the config-branch main (clap, instance, resolve, CLI dispatch, persistent cache dir) with the deploy finish: after `render`, resolve a transport and call `inkapp::publish`/`sync_once`. (Decide render-only vs publish based on a subcommand or just publish — match how main intends apps to run; at minimum wire `publish` so the deploy path is exercised, behind a clear path.)
- [ ] Port `tests/config.rs` (closes the durable cache via `close()`).
- [ ] **Verify:** `nix develop -c cargo test -p reading-queue` + clippy.
- [ ] **Commit:** "reading-queue: config-driven wiring + finished deploy path + config CLI (Spec #13 task 8)".

---

### Task 9: Migrate `agenda` (config-driven; ICS url from config)

**Goal:** Same as Task 8 for agenda (two connectors bound by `ConnectorRef`; ICS URL from config; preserve the mode-axis view).

**Files:** Modify `apps/agenda/{Cargo.toml,src/main.rs,src/lib.rs}`.

**Acceptance:**
- [ ] `AppConfig` (kind=agenda) with `device_folder` + `feed`/`cal` `ConnectorRef`s; sync `Connectors::from_config` (require_instance both; propagates LocalCal's empty-path error).
- [ ] `main()` mirrors reading-queue (instance, config CLI, resolve, transport, publish/sync_once, `.page`).
- [ ] `nix develop -c cargo test -p agenda` passes (incl. the NoSuchInstance attribution tests).

**Steps:**
- [ ] Port `AppConfig` + `Connectors::from_config` from `git show config:apps/agenda/src/lib.rs` onto main's agenda lib.rs (preserve main's `CalendarView` mode-axis view).
- [ ] Rewrite `main.rs` like reading-queue (agenda's `from_config` is synchronous).
- [ ] Port `tests/config.rs` (both wiring + per-binding NoSuchInstance attribution tests).
- [ ] **Verify:** `nix develop -c cargo test -p agenda` + clippy.
- [ ] **Commit:** "agenda: config-driven wiring (ICS url from config) + config CLI (Spec #13 task 9)".

---

### Task 10: `.gitignore`, device tests, dead-deploy cleanup

**Goal:** Replace `deploy.toml` artifacts with `config.toml`; update the manual device tests.

**Files:** `.gitignore`, `apps/*/tests/device.rs`, any lingering `deploy.toml` references.

**Acceptance:**
- [ ] `.gitignore`: `/apps/*/deploy.toml` → `/apps/*/config.toml` (or a single ignored `config.toml`); also ignore the per-app overlay JSONs if still used.
- [ ] `apps/*/tests/device.rs` updated: no `INKAPP_DEPLOY_CONFIG`; use `INKAPP_CONFIG_PATH` + an `--instance`/config fixture, or mark clearly as manual. No references to removed `serve.rs`/`DeployConfig`.
- [ ] `nix develop -c cargo test -p reading-queue -p agenda` (device tests are `#[ignore]`; just must compile).

**Steps:**
- [ ] Update `.gitignore`.
- [ ] Update both `tests/device.rs` to drive deployment via the new transport + config path (they call `inkapp::publish`/`sync_once` with a transport, or the app's wiring). Keep them `#[ignore]`.
- [ ] Grep for `INKAPP_DEPLOY_CONFIG`, `DeployConfig`, `deploy.toml`, `serve::` across the tree; remove/redirect stragglers.
- [ ] **Verify + commit:** "apps: config.toml gitignore + device tests on the config-driven path (Spec #13 task 10)".

---

### Task 11: Renumber spec/plan to #13 + reconcile appdx

**Goal:** Bring the design spec + this integration over, renumber off #12, and make `docs/appdx.md` true on main.

**Files:** `docs/superpowers/specs/2026-05-25-configuration-design.md` (+ rename/renumber), this plan, `docs/appdx.md`.

**Acceptance:**
- [ ] The configuration design spec is present on the integration branch, renumbered **Spec #12 → Spec #13** in its header and any internal references; the original config-branch plan is superseded by this integration plan.
- [ ] `docs/appdx.md` "Secrets & config" describes `inkapp-config` (config.toml, derive+registry, instances, SecretRef/ConnectorRef, CLI) AND the **folded deploy** (`[device].backend` + per-app `device_folder` driving `resolve_transport`); the prior `deploy.toml` description is replaced. Marked built (Spec #13).
- [ ] No remaining appdx text claims `deploy.toml` or "no end-user config".

**Steps:**
- [ ] Copy `git show config:docs/superpowers/specs/2026-05-25-configuration-design.md` in; change "Spec #12" → "Spec #13" (header + body). Add a short note that it subsumes main's `deploy.toml`.
- [ ] Reconcile appdx: merge the config-branch "Secrets & config" rewrite (`git show config:docs/appdx.md`) with main's appdx (which has deployment + image-pipeline sections). Update the deployment paragraph to say deploy config now lives in `config.toml` `[device]`/`device_folder`, resolved to a `DeviceTransport`.
- [ ] **Commit:** "docs: configuration spec #13 + appdx reconcile (config subsumes deploy.toml) (Spec #13 task 11)".

---

### Task 12: Workspace verification + Cargo.lock

**Goal:** Whole workspace green; lockfile reconciled.

**Acceptance:**
- [ ] `nix develop -c cargo test --workspace` passes.
- [ ] `nix develop -c cargo clippy --all-targets -- -D warnings` clean.
- [ ] `nix develop -c cargo fmt --check` clean.
- [ ] `Cargo.lock` committed separately; `nix develop -c cargo build --offline --locked` succeeds (guard against the silent lockfile desync noted in project memory — see the toml-version disambiguation from main commit `7e597d3`).

**Steps:**
- [ ] Run the three gates; fix any integration fallout (most likely: app view/Article wiring, builder `.page()`, transport signatures).
- [ ] `nix develop -c cargo build` to settle `Cargo.lock`; then `git add Cargo.lock && git commit -m "Cargo.lock: config deps for Spec #13 integration"`.
- [ ] `nix develop -c cargo build --offline --locked` to confirm no desync.
- [ ] **Commit** any fmt/clippy fixes.

---

## Self-Review

**Coverage:** Every config-branch deliverable is re-landed — both config crates (T1), PageConfig+builder (T2), the three connector `from_config`s (T3–T5), facade (T6), the CLI (rides T1, exposed via T6), both app migrations (T8–T9). The new integration-specific work is the deploy fold (T7), the finished deploy wiring (T8–T9), gitignore/device-test cleanup (T10), renumber+appdx (T11), and verification (T12). main's architecture (DeviceTransport/sync/rm-device) and content/image pipeline are adopted/preserved, not reworked.

**Risks called out:** (a) main's `App` builder may lack `.page()` — T2 restores it; verify `App::new`'s current arity. (b) App `lib.rs` must keep main's `inkapp_content::Article` rendering + `image_urls` — T8/T9 add config *around* it, never replace it. (c) `publish`/`sync_once` set-ownership: preserve main's `DocSet` handling, only inject the transport. (d) Cargo.lock silent desync — T12 guards with `--offline --locked`.

**Not re-pasting code:** the implementation is the reviewed config-branch source at `c10e2f1`; tasks reference `git show config:<path>` and list the main-specific adaptations. Each task still has concrete acceptance + verify commands.
