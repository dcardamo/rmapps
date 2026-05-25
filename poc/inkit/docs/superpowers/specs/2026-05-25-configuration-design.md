# inkapp — Spec #13: End-user configuration (config store + typed registry + CLI)

**Date:** 2026-05-25
**Status:** Approved (design); plan pending

> **Subsumes main's `deploy.toml`.** Deployment config (device backend + on-device
> target folder) is no longer a separate `deploy.toml` file: it folds into this spec's
> `config.toml` as the framework `[device].backend` section plus each app instance's
> `device_folder` key.

## Context

Configuration is one of the primary ways an **end user** (a device owner running an
installed inkapp app) interacts with the framework. Today that interaction barely exists.

Configuration is currently split in two, with a hole in the middle:

| Kind | Mechanism | Who sets it | When |
|-----------------------|----------------------------------------------------------|--------------------|------------|
| Secrets | `SecretStore` JSON (`$XDG_CONFIG_HOME/inkapp/secrets.json`, 0600) | end user (hand-edit) | runtime |
| Everything non-secret | Rust constants / `Default` impls (`ReaderConfig`, `PageGeom`, `FOLDER`, ICS feed URL) | **app developer, at compile time** | build time |

The hole: there is **no end-user-facing non-secret configuration**. Concretely:

- The ICS connector's feed URL is a **committed fixture** (`IcsConnector::from_fixture()`).
  A user cannot point the agenda app at their own calendar without recompiling.
- Reader locations/caps (`ReaderConfig`), the on-device folder (`FOLDER = "/ReadingQueue"`),
  page geometry (`PageGeom` 420×560), overlay file paths — all baked into the binary.
- `SecretStore` is the only thing a user edits at runtime, and it is secrets-only by design.

This spec adds the missing layer: a typed, end-user-facing **configuration store** that a
device owner edits without touching code, plus the framework machinery and CLI to drive it.

### What this spec makes true

- A shared, human-editable `config.toml`, **separate from `secrets.json`**, where config
  **references secrets by name, never by value**.
- **Typed, app/connector-declared** config: each connector and app declares a config struct;
  the framework loads, validates, and resolves it with precise errors.
- A **`#[derive(Config)]` proc-macro + per-binary schema registry** (Approach C) so the CLI
  can introspect the schema — list keys, show defaults, generate a starter file — and so a
  future on-device config surface can render from the same registry.
- **Named instances in the one file, selected at launch** — the same app can run multiple
  times with different config (`[app.agenda.work]`, `[app.agenda.personal]`).
- **Two editing surfaces now**: hand-edit the file, and `config` **CLI subcommands**. (An
  on-device config document is a deliberate future, enabled by the registry — see below.)
- Migration of `reading-queue` and `agenda` to config-driven wiring, closing the ICS-URL hole.

### Explicitly out of scope

- **On-device config document.** The most on-brand surface (config as an inkapp document the
  user annotates) requires a level of bootstrap that does not exist yet — you must be able to
  render and sync before you can configure by rendering. Captured as the motivating future for
  Approach C (the registry is the substrate it would render from); not built here.
- **Hot reload / watch.** Config is read **once at launch** into an immutable snapshot; the
  running sync loop holds that snapshot. Changing config requires a restart. A future
  watch/atomic-swap layer is not precluded but is not built.
- **Cross-instance shared connector caches.** Instance selection and binding are in scope;
  the cache-sharing semantics across two instances of the same connector remain as today
  (per appdx: connectors/caches are shared across a user's *apps*, never across users).
- **At-rest secret protection / KMS / rotation / tenant isolation.** Unchanged from the
  secrets spec; stays in `FUTURE.md` and the appdx threat model.

### Position in the spec arc

Specs #1–#12 built the loop spine (Typst readback, deterministic harness, gesture fixtures,
MVU loop, secrets+encryption, connector trait, mode axis, Typst authoring, document state,
pagination, live Readwise connector + durable cache, multipage device pull). This is
**Spec #13** — the first spec whose primary audience is the *end user* rather than the app
developer or the framework.

## Decisions (settled during design)

1. **Two files, one rule.** `secrets.json` stays exactly as-is. A new `config.toml` lives
   beside it. Config points at secrets *by name* (the indirection `Readwise::live` already
   uses: `ConnectorCred "readwise-reader"`). Rationale: the appdx threat model is
   "secrets never leak"; co-locating invites accidental disclosure (a user sharing config, a
   dotfiles commit). Separation lets config be freely shareable while secrets never leave the
   machine. Formats also want to differ (readable commented TOML vs 0600 base64) and
   permissions differ (0644 vs 0600).
2. **Typed, app/connector-declared schema** (not freeform key-value).
3. **Approach C — derive macro + registry.** Richest CLI introspection and the substrate for
   future on-device rendering. Upgrade path is clean: no proc-macro consumers change if the
   registry mechanism evolves.
4. **One shared file, named instances, selected at launch.**
5. **Explicit connector binding.** An app instance names the connector instances it binds
   (`feed = "ics.work"`), because convention-by-name breaks the moment one app binds two
   connectors of the same kind.
6. **Read once at launch (snapshot).**
7. **Path is a parameter**, mirroring `SecretStore`: `ConfigStore::open(path)` for tests,
   `open_default()` resolving `$INKAPP_CONFIG_PATH` → `$XDG_CONFIG_HOME/inkapp/config.toml`
   → `$HOME/.config/inkapp/config.toml`.

## 1. The shape on disk

Two files under `$XDG_CONFIG_HOME/inkapp/`:

- `secrets.json` — unchanged. 0600, machine-managed, base64.
- `config.toml` — new. 0644, hand-editable, commented, shareable.

```toml
# Framework defaults (overridable per app instance)
[page]
width = 420
height = 560
margin = 16

# Connector instances — shared across apps, keyed [connector.<kind>.<instance>]
[connector.readwise.main]
token = "readwise-reader"              # SecretRef → name in secrets.json
library_locations = ["new", "later", "shortlist"]
library_max = 100
feed_enabled = true
feed_max = 100

[connector.ics.work]
url = "https://example.com/work.ics"

[connector.localcal.work]
store_path = "~/.local/share/inkapp/work-cal.json"

# App instances — keyed [app.<kind>.<instance>], bind connectors explicitly
[app.agenda.work]
feed = "ics.work"                       # ConnectorRef
cal  = "localcal.work"                  # ConnectorRef
device_folder = "/Agenda-Work"

[app.agenda.personal]
feed = "ics.personal"
cal  = "localcal.personal"
device_folder = "/Agenda-Personal"
```

### Two typed reference fields

The cross-cutting rules are carried by types so they are enforceable, not conventional:

- **`SecretRef(String)`** — a name resolved against `SecretStore` at construction. Never holds
  a value in config. The registry marks `SecretRef` fields so the CLI never prints a resolved
  value and `config template` emits a placeholder.
- **`ConnectorRef { kind, instance }`** — binds an app instance to a connector instance.
  Validated against the loaded file; a missing target is a loud error that lists available
  instances of that kind.

Both types live in `inkapp-config` and implement serde so they deserialize from plain TOML
strings (`"readwise-reader"`, `"ics.work"`).

## 2. Derive macro + registry (Approach C)

### Crates

- **`inkapp-config`** (new lib crate) — the `ConfigStore`, the registry, `ConfigSchema`
  descriptor, `SecretRef`/`ConnectorRef`, and the `Config` trait. Depends only on
  `serde`/`toml`/`toml_edit`/`inventory` — **no device or `inkapp-core` deps**, so both
  `inkapp-core` and connector crates can depend on it without cycles.
- **`inkapp-config-derive`** (new proc-macro crate) — provides `#[derive(Config)]`.

### `#[derive(Config)]`

On a struct, the derive:

- implements the `Config` trait (associated `KIND: &'static str` and
  `NAMESPACE: Namespace` — `Connector` or `App` — set via
  `#[config(kind = "readwise", namespace = "connector")]`), so the store knows whether a kind
  resolves under `[connector.*]` or `[app.*]`,
- derives serde `Deserialize` and a `Default` (per-field defaults via
  `#[config(default = "...")]`, falling back to `Default::default()`),
- captures per-field metadata — name, declared type (as a string), default rendering, the
  `///` doc comment, and `SecretRef`/`ConnectorRef` markers,
- emits an `inventory::submit!` registering a `ConfigSchema` descriptor under `KIND`.

```rust
#[derive(Config)]
#[config(kind = "readwise")]
pub struct ReaderConfig {
    /// Reader locations that make up the Library view, in order.
    #[config(default = r#"["new","later","shortlist"]"#)]
    pub library_locations: Vec<Location>,
    #[config(default = "100")]
    pub library_max: usize,
    pub feed_enabled: bool,
    #[config(default = "100")]
    pub feed_max: usize,
    /// Readwise API token (name in the secret store).
    pub token: SecretRef,
}
```

### Registry

The registry collects all `ConfigSchema`s via `inventory`. It is **per-binary** — a binary
sees only the schemas of the crates it links. For the shared file this is exactly right: each
app's CLI shows its own surface, and sections owned by other apps are simply unknown (and
ignored — see §6). `inventory` is the registration mechanism (`linkme` is a viable
alternative if `inventory` proves problematic on the cross-compiled ARM target; the choice is
an implementation detail behind the registry API).

## 3. `ConfigStore` (load + resolve)

In `inkapp-config`, mirroring `SecretStore`:

- `open(path)` / `open_default()` with `$INKAPP_CONFIG_PATH` → `$XDG_CONFIG_HOME/inkapp/config.toml`
  → `$HOME/.config/inkapp/config.toml` resolution.
- A missing file → empty (all defaults), same as secrets.
- Parses once into a `toml_edit::Document` (kept for the CLI's comment-preserving writes) and
  resolves typed sections on demand.
- `resolve::<T: Config>(instance: &str) -> Result<T>` — typed-loads
  `[<T::NAMESPACE>.<T::KIND>.<instance>]` (e.g. `[connector.readwise.main]`), applies defaults
  for absent keys, and treats
  **unknown keys within a known section** as errors (typo protection) while **ignoring unknown
  sections** (another app owns them in the shared file).
- Snapshot semantics: read once at launch; the running loop holds the immutable resolved value.

## 4. Builder + connector/app integration

- The **`Connector` trait** gains an associated `Config: Config` and a
  `from_config(cfg, &SecretStore) -> Result<Self>` constructor. Today's
  `Readwise::live(...)`/`persisted(...)` collapse into config-driven construction; the
  cassette/`fake` constructors are retained for tests.
- Each app's **`Connectors`** gains `from_config(&ConfigStore, instance, &SecretStore)`: it
  reads its app-instance `ConnectorRef`s and constructs each bound connector instance from its
  `[connector.<kind>.<instance>]` config.
- The **`App` builder** takes page geometry from `[page]` (overridable per app instance) and
  device folder from the resolved `[app.<kind>.<instance>]`, replacing the constants. The
  `serve.rs` `FOLDER` const becomes the configured `device_folder`.
- **Instance selection** at launch: `--instance <name>` (clap), `INKAPP_INSTANCE` fallback,
  else `"default"`.

## 5. CLI (`config` subcommands)

A reusable `clap` subcommand module each app binary mounts, so `reading-queue config …` and
`agenda config …` both work, each scoped to its linked registry.

| Command | Behavior |
|---------------------------|----------------------------------------------------------------------|
| `config path` | Print the resolved config file path |
| `config template` | Emit a starter file from the registry — all sections, defaults, `///` docs as comments, secret fields as placeholders |
| `config describe [kind]` | List sections/keys/types/defaults/docs from the registry |
| `config validate` | Parse + validate against the registry; errors with `file:line` + key path |
| `config get <key>` | Read a value |
| `config set <key> <val>` | Write via `toml_edit` (preserves comments/formatting) |
| `config edit` | Open `$EDITOR` on the file |

`set` uses `toml_edit` for read-modify-write so hand-written comments and formatting survive;
typed loads use serde. (`toml` serialization would discard comments — `toml_edit` is required
for the mutation path.)

## 6. Error posture

- Malformed TOML / type mismatch → loud at startup, with `file:line` + key path.
- Unknown key in a **known** section → error (typo protection). Unknown **section** → ignored
  (the shared file holds other apps' sections; a per-binary registry only knows its own).
- Required field with no default (e.g. ICS `url`, or a `SecretRef` whose secret is absent in
  `SecretStore`) → loud error at construction naming the key, the instance, and how to set it.
- Missing `ConnectorRef` target → loud error listing the available instances of that kind.

## 7. Testing

- `ConfigStore` round-trip on temp files; defaults applied for absent keys; `$INKAPP_CONFIG_PATH`
  override honored; missing file → defaults.
- Registry: a `#[derive(Config)]` struct's schema (fields, types, defaults, docs, ref markers)
  is discoverable and asserts correctly.
- `config template` output re-parses to defaults; `config validate` catches unknown-key and
  type errors and **ignores foreign sections**.
- `SecretRef` never serializes with a value and resolves via `SecretStore`; `ConnectorRef`
  resolves to an existing instance and errors (with available instances) on a missing target.
- Instance selection precedence: `--instance` > `INKAPP_INSTANCE` > `"default"`.
- Per-binary registry scoping (a binary sees only its linked schemas).
- **Migration:** `reading-queue` and `agenda` build and wire from config; existing app/device
  tests pass; agenda's ICS URL now comes from config (the committed fixture is retained for
  tests only, closing the "fixture is the only way" hole).

## 8. Definition of done

Update `docs/appdx.md` "Secrets & config" to describe the config store, the derive + registry,
named instances, `SecretRef`/`ConnectorRef`, and the CLI — and mark it built (per the
project's appdx-is-the-definition-of-done rule).

## Scope note (for planning)

This is sizable — a proc-macro crate, a registry crate, the store, two ref types, the
`Connector`/builder integration, a CLI, and migrating two apps. It is one coherent spec, but
the implementation plan should phase it:

1. `inkapp-config` + `inkapp-config-derive`: `ConfigStore`, `Config` trait, registry,
   `SecretRef`/`ConnectorRef`, `#[derive(Config)]`. (Self-contained; unit-tested in isolation.)
2. `Connector::from_config` + builder integration (page/device-folder from config).
3. `config` CLI subcommands.
4. Migrate `reading-queue` + `agenda`; reconcile `appdx`.

**New dependencies:** `inkapp-config`, `inkapp-config-derive` (workspace crates); `inventory`
and `clap` (external).
