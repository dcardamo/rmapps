# rmbujo Phase 1 Implementation Plan (Rust)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `rmbujo` Rust CLI that generates a year of dot-grid bullet-journal PDFs for the reMarkable Paper Pro Move (and Paper Pro), driven by a per-year TOML config, rendered via fulgur (no browser).

**Architecture:** Pure-data modules (device, calendar, config, theme, geometry, svg) feed askama HTML templates; `render.rs` assembles the HTML + generated SVG assets + embedded font and calls fulgur (Blitz+krilla) to produce one byte-deterministic PDF per notebook. Phase 2 seams (ICS config, `Deployer` trait) are present but inert. All deps via a Nix flake + direnv.

**Tech Stack:** Rust 1.9x, fulgur 0.6, askama 0.13, serde+toml, chrono, clap, dialoguer, anyhow; dev: lopdf, image; poppler (`pdftoppm`) + libiconv via Nix.

**User Verification:** NO — no user verification required. The spec mandates fully automated testing; the visual design was validated by the committed spike and approved mockups.

---

## Spike-derived constraints (already proven — see `docs/superpowers/spikes/2026-05-20-fulgur-spike.md`)

- fulgur renders flex/text/pills/custom-page-size and is byte-deterministic.
- **CSS gradients are NOT painted** → dot grid + cover ship as **generated SVG assets**.
- Nix-on-macOS needs **`libiconv`** in the dev shell to link.
- `fulgur::inspect::inspect(pdf)` yields laid-out `TextItem { x,y,width,height,text }` → used for **geometric overlap/bounds tests**.

## File Structure

See the spec's "Code architecture" section. Crate is **lib + bin** (`src/lib.rs` + `src/main.rs`) so integration tests in `tests/` can use the public API. Templates live in `templates/` (askama default). The default theme and the font are **embedded** via `include_str!` / `include_bytes!` so the binary is self-contained. The orchestrator is `generate.rs` (NOT `build.rs`, which Cargo reserves).

---

### Task 0: Nix flake, Cargo scaffold, vendored font, fulgur smoke test

**Goal:** A Nix dev shell where the crate builds/links and fulgur renders a PDF.

**Files:**
- Create: `flake.nix`, `.envrc`, `Cargo.toml`, `Makefile`, `.gitignore`
- Create: `src/lib.rs`, `src/main.rs`, `assets/fonts/DejaVuSerif.ttf`, `assets/fonts/DejaVuSerif-Bold.ttf`
- Create: `tests/smoke.rs`

**Acceptance Criteria:**
- [ ] `nix develop -c cargo test --test smoke` passes (fulgur renders a PDF; links with libiconv).
- [ ] `direnv allow` loads the shell; `pdftoppm` is on PATH inside it.

**Verify:** `nix develop -c cargo test --test smoke` → ok. 1 passed

**Steps:**

- [ ] **Step 1: Write `flake.nix`**

```nix
{
  description = "rmbujo — dot-grid bullet journal PDF generator for reMarkable";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = import nixpkgs { inherit system; };
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.pkg-config ];
          buildInputs = [ pkgs.libiconv pkgs.fontconfig pkgs.poppler_utils pkgs.dejavu_fonts ];
        };
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "rmbujo";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.libiconv pkgs.fontconfig ];
        };
      });
}
```

- [ ] **Step 2: Write `.envrc`**

```bash
use flake
```

- [ ] **Step 3: Write `.gitignore`** (replace the Python one)

```gitignore
/target
**/*.rs.bk
```

- [ ] **Step 4: Write `Cargo.toml`**

```toml
[package]
name = "rmbujo"
version = "0.1.0"
edition = "2021"
description = "Dot-grid bullet journal PDF generator for reMarkable"

[lib]
name = "rmbujo"
path = "src/lib.rs"

[[bin]]
name = "rmbujo"
path = "src/main.rs"

[dependencies]
fulgur = "0.6"
askama = "0.13"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
clap = { version = "4", features = ["derive"] }
dialoguer = "0.11"
anyhow = "1"

[dev-dependencies]
lopdf = "0.36"
image = "0.25"
```

- [ ] **Step 5: Write `Makefile`** (tab-indented recipes)

```make
.PHONY: test build update-goldens fmt clippy
test:
	nix develop -c cargo test
build:
	nix build
update-goldens:
	nix develop -c env RMBUJO_UPDATE_GOLDENS=1 cargo test --test visual
fmt:
	nix develop -c cargo fmt
clippy:
	nix develop -c cargo clippy -- -D warnings
```

- [ ] **Step 6: Placeholder lib + bin**

`src/lib.rs`:
```rust
//! rmbujo — dot-grid bullet journal PDF generator for reMarkable.
```
`src/main.rs`:
```rust
fn main() {
    println!("rmbujo");
}
```

- [ ] **Step 7: Vendor the font** (run inside the dev shell)

```bash
nix develop -c bash -c '
  mkdir -p assets/fonts
  d=$(nix build --no-link --print-out-paths nixpkgs#dejavu_fonts)/share/fonts/truetype
  cp "$d/DejaVuSerif.ttf" "$d/DejaVuSerif-Bold.ttf" assets/fonts/'
```
Confirm: `ls -la assets/fonts/` shows both TTFs.

- [ ] **Step 8: Write the smoke test** `tests/smoke.rs`

```rust
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

#[test]
fn fulgur_renders_pdf() {
    let engine = Engine::builder()
        .page_size(PageSize { width: 260.18, height: 462.55 })
        .margin(Margin::uniform(0.0))
        .build();
    let pdf = engine.render_html("<h1>rmbujo</h1>").expect("render");
    assert!(pdf.len() > 100, "expected a non-trivial PDF, got {} bytes", pdf.len());
}
```
> Note: `tests/smoke.rs` uses `fulgur` directly, so add `fulgur` to `[dev-dependencies]` as well (or it is reachable because it is a normal dependency only from `src/`). Simplest: add `fulgur = "0.6"` under `[dev-dependencies]` too — Cargo dedups the build.

- [ ] **Step 9: Generate the lockfile and run smoke**

Run: `nix develop -c cargo generate-lockfile`
Run: `nix develop -c cargo test --test smoke`
Expected: `test fulgur_renders_pdf ... ok`. If linking fails with `-liconv`, confirm `libiconv` is in `buildInputs` and you are inside `nix develop`.

- [ ] **Step 10: Commit**

```bash
git add flake.nix flake.lock .envrc .gitignore Cargo.toml Cargo.lock Makefile src/ assets/ tests/smoke.rs
git commit -m "Add Nix flake, Cargo scaffold, vendored font, fulgur smoke test"
```

```json:metadata
{"files": ["flake.nix", ".envrc", "Cargo.toml", "Makefile", "assets/fonts/DejaVuSerif.ttf", "tests/smoke.rs"], "verifyCommand": "nix develop -c cargo test --test smoke", "acceptanceCriteria": ["fulgur renders a PDF (links with libiconv)", "pdftoppm available in shell"], "requiresUserVerification": false}
```

---

### Task 1: Device geometry (`src/device.rs`)

**Goal:** Map a device key to page size in points.

**Files:** Create `src/device.rs`; modify `src/lib.rs`; create `tests/device.rs`

**Acceptance Criteria:**
- [ ] `get_device("paper-pro-move")` → width ≈ 260.18 pt, height ≈ 462.55 pt.
- [ ] Unknown key returns `Err`.

**Verify:** `nix develop -c cargo test --test device` → all pass

**Steps:**

- [ ] **Step 1: Write the failing test** `tests/device.rs`

```rust
use rmbujo::device::{get_device, MOVE, PRO};

fn approx(a: f32, b: f32) -> bool { (a - b).abs() < 0.01 }

#[test]
fn move_page_size() {
    let d = get_device("paper-pro-move").unwrap();
    assert!(approx(d.width_pt(), 260.18));
    assert!(approx(d.height_pt(), 462.55));
}

#[test]
fn pro_page_size() {
    let d = get_device("paper-pro").unwrap();
    assert!(approx(d.width_pt(), 509.34));
    assert!(approx(d.height_pt(), 679.13));
}

#[test]
fn unknown_device_errors() {
    assert!(get_device("nope").is_err());
}

#[test]
fn constants_present() {
    assert_eq!(MOVE.key, "paper-pro-move");
    assert_eq!(PRO.key, "paper-pro");
}
```

- [ ] **Step 2: Run → fail** (`cargo test --test device`): unresolved module `device`.

- [ ] **Step 3: Write `src/device.rs`**

```rust
//! reMarkable device specs → page geometry in PDF points (72 pt/inch).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Device {
    pub key: &'static str,
    pub name: &'static str,
    pub width_px: u32,  // short edge (portrait)
    pub height_px: u32, // long edge (portrait)
    pub ppi: u32,
}

impl Device {
    pub fn width_pt(&self) -> f32 {
        self.width_px as f32 / self.ppi as f32 * 72.0
    }
    pub fn height_pt(&self) -> f32 {
        self.height_px as f32 / self.ppi as f32 * 72.0
    }
}

pub const MOVE: Device = Device {
    key: "paper-pro-move",
    name: "reMarkable Paper Pro Move",
    width_px: 954,
    height_px: 1696,
    ppi: 264,
};

pub const PRO: Device = Device {
    key: "paper-pro",
    name: "reMarkable Paper Pro",
    width_px: 1620,
    height_px: 2160,
    ppi: 229,
};

pub fn get_device(key: &str) -> anyhow::Result<Device> {
    match key {
        "paper-pro-move" => Ok(MOVE),
        "paper-pro" => Ok(PRO),
        other => anyhow::bail!("unknown device {other:?}; choices: paper-pro-move, paper-pro"),
    }
}
```

- [ ] **Step 4: Add to `src/lib.rs`**

```rust
//! rmbujo — dot-grid bullet journal PDF generator for reMarkable.
pub mod device;
```

- [ ] **Step 5: Run → pass.** `nix develop -c cargo test --test device`

- [ ] **Step 6: Commit**

```bash
git add src/device.rs src/lib.rs tests/device.rs
git commit -m "Add device geometry"
```

```json:metadata
{"files": ["src/device.rs", "src/lib.rs", "tests/device.rs"], "verifyCommand": "nix develop -c cargo test --test device", "acceptanceCriteria": ["move ~260.18x462.55pt", "unknown device errors"], "requiresUserVerification": false}
```

---

### Task 2: Calendar (`src/calendar.rs`)

**Goal:** Per-month day lists with weekday abbreviations and week-grouping flags.

**Files:** Create `src/calendar.rs`; modify `src/lib.rs`; create `tests/calendar.rs`

**Acceptance Criteria:**
- [ ] 2026-05-18 is a Monday; May = 31 days; Feb 2024 = 29, Feb 2026 = 28.
- [ ] `week_start="sun"` → May 2026 week-starts on 3,10,17,24,31; `"mon"` → 4,11,18,25.

**Verify:** `nix develop -c cargo test --test calendar` → all pass

**Steps:**

- [ ] **Step 1: Write the failing test** `tests/calendar.rs`

```rust
use rmbujo::calendar::{build_month, build_year, MONTH_NAMES};

#[test]
fn may_2026_basics() {
    let m = build_month(2026, 5, "sun").unwrap();
    assert_eq!(m.name, "May");
    assert_eq!(m.days.len(), 31);
    assert_eq!(m.days[17].day, 18);
    assert_eq!(m.days[17].weekday, "Mon");
}

#[test]
fn february_leap() {
    assert_eq!(build_month(2024, 2, "sun").unwrap().days.len(), 29);
    assert_eq!(build_month(2026, 2, "sun").unwrap().days.len(), 28);
}

#[test]
fn week_start_sunday() {
    let m = build_month(2026, 5, "sun").unwrap();
    let starts: Vec<u32> = m.days.iter().filter(|d| d.week_start).map(|d| d.day).collect();
    assert_eq!(starts, vec![3, 10, 17, 24, 31]);
}

#[test]
fn week_start_monday() {
    let m = build_month(2026, 5, "mon").unwrap();
    let starts: Vec<u32> = m.days.iter().filter(|d| d.week_start).map(|d| d.day).collect();
    assert_eq!(starts, vec![4, 11, 18, 25]);
}

#[test]
fn year_has_12_months() {
    let y = build_year(2026, "sun").unwrap();
    assert_eq!(y.len(), 12);
    assert_eq!(MONTH_NAMES[5], "May");
}

#[test]
fn bad_week_start_errors() {
    assert!(build_month(2026, 5, "xyz").is_err());
}
```

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Write `src/calendar.rs`**

```rust
//! Calendar data: year → months → days, with weekday labels and week grouping.

use chrono::{Datelike, NaiveDate, Weekday};

pub const MONTH_NAMES: [&str; 13] = [
    "", "January", "February", "March", "April", "May", "June", "July", "August",
    "September", "October", "November", "December",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub day: u32,
    pub weekday: &'static str,
    pub week_start: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Month {
    pub year: i32,
    pub month: u32,
    pub name: &'static str,
    pub days: Vec<Day>,
}

fn weekday_abbr(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .unwrap()
        .pred_opt()
        .unwrap()
        .day()
}

fn week_start_weekday(week_start: &str) -> anyhow::Result<Weekday> {
    match week_start {
        "sun" => Ok(Weekday::Sun),
        "mon" => Ok(Weekday::Mon),
        other => anyhow::bail!("week_start must be 'sun' or 'mon', got {other:?}"),
    }
}

pub fn build_month(year: i32, month: u32, week_start: &str) -> anyhow::Result<Month> {
    let ws = week_start_weekday(week_start)?;
    let n = days_in_month(year, month);
    let mut days = Vec::with_capacity(n as usize);
    for d in 1..=n {
        let date = NaiveDate::from_ymd_opt(year, month, d).unwrap();
        let wd = date.weekday();
        days.push(Day {
            day: d,
            weekday: weekday_abbr(wd),
            week_start: d != 1 && wd == ws,
        });
    }
    Ok(Month { year, month, name: MONTH_NAMES[month as usize], days })
}

pub fn build_year(year: i32, week_start: &str) -> anyhow::Result<Vec<Month>> {
    (1..=12).map(|m| build_month(year, m, week_start)).collect()
}
```

- [ ] **Step 4: Add `pub mod calendar;` to `src/lib.rs`.**

- [ ] **Step 5: Run → pass.**

- [ ] **Step 6: Commit**

```bash
git add src/calendar.rs src/lib.rs tests/calendar.rs
git commit -m "Add calendar with week grouping"
```

```json:metadata
{"files": ["src/calendar.rs", "src/lib.rs", "tests/calendar.rs"], "verifyCommand": "nix develop -c cargo test --test calendar", "acceptanceCriteria": ["weekdays + leap years", "sunday week-starts 3,10,17,24,31"], "requiresUserVerification": false}
```

---

### Task 3: Config (`src/config.rs`, TOML)

**Goal:** Load/dump the per-year TOML config with defaults and Phase 2 sections.

**Files:** Create `src/config.rs`; modify `src/lib.rs`; create `tests/config.rs`

**Acceptance Criteria:**
- [ ] `dump` then `load` yields an equal `Config`.
- [ ] Minimal `year = 2026` toml loads with documented defaults.
- [ ] Missing `year` errors. Unknown keys are ignored.

**Verify:** `nix develop -c cargo test --test config` → all pass

**Steps:**

- [ ] **Step 1: Write the failing test** `tests/config.rs`

```rust
use rmbujo::config::{self, Config, DeployConfig, IcsFeed};

#[test]
fn round_trip() {
    let dir = tempdir();
    let cfg = Config {
        ics: vec![IcsFeed { name: "Holidays".into(), url: "https://x/h.ics".into(), color: "brick".into() }],
        ..Config::new(2026)
    };
    let p = dir.join("rmbujo.toml");
    config::dump(&cfg, &p).unwrap();
    assert_eq!(config::load(&p).unwrap(), cfg);
}

#[test]
fn minimal_defaults() {
    let dir = tempdir();
    let p = dir.join("rmbujo.toml");
    std::fs::write(&p, "year = 2026\n").unwrap();
    let c = config::load(&p).unwrap();
    assert_eq!(c.device, "paper-pro-move");
    assert_eq!(c.week_start, "sun");
    assert_eq!(c.daily_pages, 60);
    assert_eq!(c.collection_pages, 20);
    assert_eq!(c.theme, "library");
    assert!(c.ics.is_empty());
    assert_eq!(c.deploy.backend, "none");
}

#[test]
fn missing_year_errors() {
    let dir = tempdir();
    let p = dir.join("rmbujo.toml");
    std::fs::write(&p, "device = \"paper-pro\"\n").unwrap();
    assert!(config::load(&p).is_err());
}

#[test]
fn unknown_keys_ignored() {
    let dir = tempdir();
    let p = dir.join("rmbujo.toml");
    std::fs::write(&p, "year = 2026\nbogus = 1\n").unwrap();
    assert_eq!(config::load(&p).unwrap().year, 2026);
}

// Minimal unique temp dir without an extra crate dependency.
fn tempdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-test-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}
```

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Write `src/config.rs`**

```rust
//! Per-year config: serde structs + TOML load/dump.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IcsFeed {
    pub name: String,
    pub url: String,
    #[serde(default = "default_color")]
    pub color: String,
}
fn default_color() -> String { "navy".into() }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeployConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub target_folder: String,
}
fn default_backend() -> String { "none".into() }
impl Default for DeployConfig {
    fn default() -> Self { Self { backend: "none".into(), target_folder: String::new() } }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub year: i32,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_week_start")]
    pub week_start: String,
    #[serde(default = "default_daily")]
    pub daily_pages: u32,
    #[serde(default = "default_collection")]
    pub collection_pages: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub ics: Vec<IcsFeed>,
    #[serde(default)]
    pub deploy: DeployConfig,
}
fn default_device() -> String { "paper-pro-move".into() }
fn default_week_start() -> String { "sun".into() }
fn default_daily() -> u32 { 60 }
fn default_collection() -> u32 { 20 }
fn default_theme() -> String { "library".into() }

impl Config {
    /// A config with the given year and all other fields defaulted.
    pub fn new(year: i32) -> Self {
        Config {
            year,
            device: default_device(),
            week_start: default_week_start(),
            daily_pages: default_daily(),
            collection_pages: default_collection(),
            theme: default_theme(),
            ics: Vec::new(),
            deploy: DeployConfig::default(),
        }
    }
}

pub fn load(path: &Path) -> anyhow::Result<Config> {
    let s = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&s)?)
}

pub fn dump(config: &Config, path: &Path) -> anyhow::Result<()> {
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}
```

- [ ] **Step 4: Add `pub mod config;` to `src/lib.rs`.**

- [ ] **Step 5: Run → pass.** (toml ignores unknown keys by default; missing `year` has no default → parse error.)

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/lib.rs tests/config.rs
git commit -m "Add TOML config with defaults"
```

```json:metadata
{"files": ["src/config.rs", "src/lib.rs", "tests/config.rs"], "verifyCommand": "nix develop -c cargo test --test config", "acceptanceCriteria": ["round-trip equality", "minimal defaults", "missing year errors"], "requiresUserVerification": false}
```

---

### Task 4: Theme (`src/theme.rs` + `themes/library.toml`)

**Goal:** Load a theme (embedded `library` or a path) and emit CSS custom properties.

**Files:** Create `src/theme.rs`, `themes/library.toml`; modify `src/lib.rs`; create `tests/theme.rs`

**Acceptance Criteria:**
- [ ] `load_theme("library")` returns the palette keys incl. `navy` and `cover_to`.
- [ ] `css_vars` emits `:root{--brick:#8B2E1F;...}` in sorted-key order.
- [ ] Unknown theme name errors; a `.toml` path loads.

**Verify:** `nix develop -c cargo test --test theme` → all pass

**Steps:**

- [ ] **Step 1: Write `themes/library.toml`**

```toml
navy = "#1B365D"
brick = "#8B2E1F"
ochre = "#A07E1C"
olive = "#556B2F"
rule = "#D9D6CC"
dot = "#CFCDC4"
cover_to = "#0F2444"
```

- [ ] **Step 2: Write the failing test** `tests/theme.rs`

```rust
use rmbujo::theme::{css_vars, load_theme};

#[test]
fn library_palette() {
    let t = load_theme("library").unwrap();
    assert_eq!(t.get("navy").unwrap(), "#1B365D");
    assert_eq!(t.get("cover_to").unwrap(), "#0F2444");
}

#[test]
fn css_vars_sorted() {
    let t = load_theme("library").unwrap();
    let css = css_vars(&t);
    assert!(css.starts_with(":root{"));
    assert!(css.contains("--navy:#1B365D;"));
    // BTreeMap → alphabetical: brick before navy
    let bi = css.find("--brick").unwrap();
    let ni = css.find("--navy").unwrap();
    assert!(bi < ni);
}

#[test]
fn unknown_theme_errors() {
    assert!(load_theme("nope").is_err());
}

#[test]
fn theme_path_loads() {
    let mut p = std::env::temp_dir();
    p.push(format!("rmbujo-theme-{}.toml", std::process::id()));
    std::fs::write(&p, "navy = \"#000080\"\n").unwrap();
    let t = load_theme(p.to_str().unwrap()).unwrap();
    assert_eq!(t.get("navy").unwrap(), "#000080");
}
```

- [ ] **Step 3: Write `src/theme.rs`**

```rust
//! Theme: TOML palette → map + CSS custom properties.

use std::collections::BTreeMap;

const LIBRARY_TOML: &str = include_str!("../themes/library.toml");

pub type Palette = BTreeMap<String, String>;

pub fn load_theme(name_or_path: &str) -> anyhow::Result<Palette> {
    let content = match name_or_path {
        "library" => LIBRARY_TOML.to_string(),
        p if p.ends_with(".toml") => std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("theme not found: {name_or_path} ({e})"))?,
        other => anyhow::bail!("unknown theme {other:?}; use 'library' or a path to a .toml"),
    };
    Ok(toml::from_str(&content)?)
}

pub fn css_vars(theme: &Palette) -> String {
    let mut s = String::from(":root{");
    for (k, v) in theme {
        s.push_str(&format!("--{k}:{v};"));
    }
    s.push('}');
    s
}
```

- [ ] **Step 4: Add `pub mod theme;` to `src/lib.rs`.**

- [ ] **Step 5: Run → pass.**

- [ ] **Step 6: Commit**

```bash
git add src/theme.rs themes/library.toml src/lib.rs tests/theme.rs
git commit -m "Add theme loader and Library palette"
```

```json:metadata
{"files": ["src/theme.rs", "themes/library.toml", "src/lib.rs", "tests/theme.rs"], "verifyCommand": "nix develop -c cargo test --test theme", "acceptanceCriteria": ["library palette loads", "css vars sorted", "unknown theme errors"], "requiresUserVerification": false}
```

---

### Task 5: Dot-grid geometry (`src/geometry.rs`)

**Goal:** Compute dot spacing/margin/counts for a device.

**Files:** Create `src/geometry.rs`; modify `src/lib.rs`; create `tests/geometry.rs`

**Acceptance Criteria:**
- [ ] Move default grid: spacing ≈ 14.17 pt, margin ≈ 17.01 pt, cols = 16, rows = 31.

**Verify:** `nix develop -c cargo test --test geometry` → pass

**Steps:**

- [ ] **Step 1: Write the failing test** `tests/geometry.rs`

```rust
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;

#[test]
fn move_grid() {
    let g = default_grid(&get_device("paper-pro-move").unwrap());
    assert!((g.spacing_pt - 14.17).abs() < 0.01);
    assert!((g.margin_pt - 17.01).abs() < 0.01);
    assert_eq!(g.cols, 16);
    assert_eq!(g.rows, 31);
}
```

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Write `src/geometry.rs`**

```rust
//! Dot-grid geometry derived from a device's page size.

use crate::device::Device;

const MM_PER_PT: f32 = 25.4 / 72.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridSpec {
    pub spacing_pt: f32,
    pub margin_pt: f32,
    pub cols: u32,
    pub rows: u32,
}

pub fn dot_grid(device: &Device, spacing_mm: f32, margin_mm: f32) -> GridSpec {
    let spacing_pt = spacing_mm / MM_PER_PT;
    let margin_pt = margin_mm / MM_PER_PT;
    let usable_w = device.width_pt() - 2.0 * margin_pt;
    let usable_h = device.height_pt() - 2.0 * margin_pt;
    let cols = (usable_w / spacing_pt).floor() as u32 + 1;
    let rows = (usable_h / spacing_pt).floor() as u32 + 1;
    GridSpec { spacing_pt, margin_pt, cols, rows }
}

pub fn default_grid(device: &Device) -> GridSpec {
    dot_grid(device, 5.0, 6.0)
}
```

- [ ] **Step 4: Add `pub mod geometry;` to `src/lib.rs`.**

- [ ] **Step 5: Run → pass.**

- [ ] **Step 6: Commit**

```bash
git add src/geometry.rs src/lib.rs tests/geometry.rs
git commit -m "Add dot-grid geometry"
```

```json:metadata
{"files": ["src/geometry.rs", "src/lib.rs", "tests/geometry.rs"], "verifyCommand": "nix develop -c cargo test --test geometry", "acceptanceCriteria": ["move spacing 14.17 cols 16 rows 31"], "requiresUserVerification": false}
```

---

### Task 6: SVG generators (`src/svg.rs`)

**Goal:** Generate the dot-tile and cover SVGs deterministically from theme + geometry.

**Files:** Create `src/svg.rs`; modify `src/lib.rs`; create `tests/svg.rs`

**Acceptance Criteria:**
- [ ] `dot_tile_svg` contains a `<circle>` with the dot color and the tile size.
- [ ] `cover_svg` contains a `<linearGradient>` with both stop colors and the page size.

**Verify:** `nix develop -c cargo test --test svg` → pass

**Steps:**

- [ ] **Step 1: Write the failing test** `tests/svg.rs`

```rust
use rmbujo::svg::{cover_svg, dot_tile_svg};

#[test]
fn dot_tile() {
    let s = dot_tile_svg(14.17, "#CFCDC4");
    assert!(s.contains("<circle"));
    assert!(s.contains("#CFCDC4"));
    assert!(s.contains("14.17"));
}

#[test]
fn cover() {
    let s = cover_svg(260.18, 462.55, "#1B365D", "#0F2444");
    assert!(s.contains("linearGradient"));
    assert!(s.contains("#1B365D"));
    assert!(s.contains("#0F2444"));
    assert!(s.contains("260.18"));
}
```

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Write `src/svg.rs`**

```rust
//! Generated SVG assets (fulgur 0.6 does not paint CSS gradients).

/// A single dot-grid cell, tiled via CSS `background-repeat`.
pub fn dot_tile_svg(spacing_pt: f32, dot_color: &str) -> String {
    let c = spacing_pt / 2.0;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{s}\" height=\"{s}\">\
         <circle cx=\"{c}\" cy=\"{c}\" r=\"0.7\" fill=\"{col}\"/></svg>",
        s = spacing_pt, c = c, col = dot_color,
    )
}

/// Full-page cover gradient.
pub fn cover_svg(width_pt: f32, height_pt: f32, from: &str, to: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\">\
         <defs><linearGradient id=\"g\" x1=\"0%\" y1=\"0%\" x2=\"100%\" y2=\"100%\">\
         <stop offset=\"0%\" stop-color=\"{from}\"/>\
         <stop offset=\"100%\" stop-color=\"{to}\"/></linearGradient></defs>\
         <rect width=\"{w}\" height=\"{h}\" fill=\"url(#g)\"/></svg>",
        w = width_pt, h = height_pt, from = from, to = to,
    )
}
```

- [ ] **Step 4: Add `pub mod svg;` to `src/lib.rs`.**

- [ ] **Step 5: Run → pass.**

- [ ] **Step 6: Commit**

```bash
git add src/svg.rs src/lib.rs tests/svg.rs
git commit -m "Add SVG generators for dot grid and cover"
```

```json:metadata
{"files": ["src/svg.rs", "src/lib.rs", "tests/svg.rs"], "verifyCommand": "nix develop -c cargo test --test svg", "acceptanceCriteria": ["dot tile has circle+color", "cover has gradient+colors"], "requiresUserVerification": false}
```

---

### Task 7: askama templates (`src/templates.rs` + `templates/*.html`)

**Goal:** Compile-time-checked page templates that render to HTML strings.

**Files:** Create `src/templates.rs`, `templates/{base,cover,dotgrid,tasks,month_index,future_log,reference}.html`; modify `src/lib.rs`; create `tests/templates_html.rs`

**Acceptance Criteria:**
- [ ] Month index renders one `.day` row per day with `num`/`wd`; `weekstart` class on week-start days.
- [ ] Cover with `blank_title=true` emits `title-blank` and no `class="title"`.
- [ ] Future-log page renders 3 `.fl-block`s; reference renders the legend incl. `=` and 2 sections.

**Verify:** `nix develop -c cargo test --test templates_html` → all pass

**Steps:**

- [ ] **Step 1: Write `templates/base.html`**

```html
<!doctype html><html lang="en"><head><meta charset="utf-8"><style>{{ css|safe }}</style></head>
<body>{% for page in pages %}{{ page|safe }}{% endfor %}</body></html>
```

- [ ] **Step 2: Write `templates/cover.html`**

```html
<section class="page"><div class="cover">
  <div class="year">{{ year }}</div>
  {% if blank_title %}<div class="title-blank"></div>{% else %}<div class="title">{{ title }}</div>{% endif %}
</div></section>
```

- [ ] **Step 3: Write `templates/dotgrid.html`**

```html
<section class="page"><div class="dotgrid"></div></section>
```

- [ ] **Step 4: Write `templates/tasks.html`**

```html
<section class="page"><div class="h-section">Tasks</div><div class="dotgrid dotgrid--below"></div></section>
```

- [ ] **Step 5: Write `templates/month_index.html`**

```html
<section class="page">
  <div class="h-month">{{ month_name }} {{ year }}</div>
  <div class="daylist">
    {% for day in days %}
    <div class="day{% if day.week_start %} weekstart{% endif %}">
      <span class="num">{{ day.day }}</span>
      <span class="wd">{{ day.weekday }}</span>
      <span class="gutter"></span>
    </div>
    {% endfor %}
  </div>
</section>
```

- [ ] **Step 6: Write `templates/future_log.html`**

```html
<section class="page">
  {% for name in months %}
  <div class="fl-block"><div class="h-month">{{ name }}</div></div>
  {% endfor %}
</section>
```

- [ ] **Step 7: Write `templates/reference.html`**

```html
<section class="page">
  <div class="h-section">Key</div>
  <div class="legend">
    <div><span class="sym">&bull;</span> Task</div>
    <div><span class="sym">&times;</span> Task complete</div>
    <div><span class="sym">&gt;</span> Migrated</div>
    <div><span class="sym">&lt;</span> Scheduled</div>
    <div><span class="sym">&#9675;</span> Event</div>
    <div><span class="sym">&mdash;</span> Note</div>
    <div><span class="sym">&#9733;</span> Priority</div>
    <div><span class="sym">=</span> Feeling / mood</div>
  </div>
</section>
<section class="page">
  <div class="h-section">Using this journal</div>
  <div class="legend">
    <p><b>Start a month:</b> set up the day list and the Tasks page, then migrate open
    items forward from last month and the Future Log.</p>
    <p><b>End a month:</b> review each day and the Tasks page. Complete (&times;),
    migrate (&gt;) unfinished tasks to next month, or schedule (&lt;) them into the
    Future Log. Drop what no longer matters.</p>
  </div>
</section>
```

- [ ] **Step 8: Write `src/templates.rs`**

```rust
//! askama template structs (compile-time checked). Each renders an HTML fragment.

use askama::Template;

#[derive(Clone)]
pub struct DayView {
    pub day: u32,
    pub weekday: &'static str,
    pub week_start: bool,
}

#[derive(Template)]
#[template(path = "base.html")]
pub struct Base<'a> {
    pub css: &'a str,
    pub pages: &'a [String],
}

#[derive(Template)]
#[template(path = "cover.html")]
pub struct Cover<'a> {
    pub year: i32,
    pub title: &'a str,
    pub blank_title: bool,
}

#[derive(Template)]
#[template(path = "dotgrid.html")]
pub struct DotGrid;

#[derive(Template)]
#[template(path = "tasks.html")]
pub struct Tasks;

#[derive(Template)]
#[template(path = "month_index.html")]
pub struct MonthIndex<'a> {
    pub month_name: &'a str,
    pub year: i32,
    pub days: &'a [DayView],
}

#[derive(Template)]
#[template(path = "future_log.html")]
pub struct FutureLog<'a> {
    pub months: &'a [&'a str],
}

#[derive(Template)]
#[template(path = "reference.html")]
pub struct Reference;
```

- [ ] **Step 9: Add `pub mod templates;` to `src/lib.rs`.**

- [ ] **Step 10: Write the test** `tests/templates_html.rs`

```rust
use askama::Template;
use rmbujo::calendar::build_month;
use rmbujo::templates::{Cover, DayView, FutureLog, MonthIndex, Reference};

#[test]
fn month_index_rows() {
    let m = build_month(2026, 5, "sun").unwrap();
    let days: Vec<DayView> = m.days.iter()
        .map(|d| DayView { day: d.day, weekday: d.weekday, week_start: d.week_start })
        .collect();
    let html = MonthIndex { month_name: "May", year: 2026, days: &days }.render().unwrap();
    assert_eq!(html.matches("class=\"day").count(), 31);
    assert!(html.contains("weekstart"));
    assert!(html.contains(">18<") && html.contains("Mon"));
}

#[test]
fn cover_blank_vs_titled() {
    let blank = Cover { year: 2026, title: "", blank_title: true }.render().unwrap();
    assert!(blank.contains("title-blank"));
    assert!(!blank.contains("class=\"title\""));
    let titled = Cover { year: 2026, title: "Reference", blank_title: false }.render().unwrap();
    assert!(titled.contains("Reference"));
}

#[test]
fn future_log_blocks() {
    let html = FutureLog { months: &["January", "February", "March"] }.render().unwrap();
    assert_eq!(html.matches("fl-block").count(), 3);
    assert!(html.contains("February"));
}

#[test]
fn reference_legend() {
    let html = Reference.render().unwrap();
    assert!(html.contains("Feeling / mood"));
    assert_eq!(html.matches("class=\"page\"").count(), 2);
}
```

- [ ] **Step 11: Run → pass.** (askama compiles templates at build time; a template typo is a build error.)

- [ ] **Step 12: Commit**

```bash
git add src/templates.rs templates/ src/lib.rs tests/templates_html.rs
git commit -m "Add askama templates for all page types"
```

```json:metadata
{"files": ["src/templates.rs", "templates/base.html", "templates/cover.html", "templates/month_index.html", "templates/future_log.html", "templates/reference.html", "templates/dotgrid.html", "templates/tasks.html", "tests/templates_html.rs"], "verifyCommand": "nix develop -c cargo test --test templates_html", "acceptanceCriteria": ["31 day rows + weekstart", "cover blank vs titled", "3 fl-blocks", "reference legend incl = and 2 sections"], "requiresUserVerification": false}
```

---

### Task 8: Rendering (`src/render.rs`)

**Goal:** Assemble HTML + CSS + SVG assets + embedded font and render to PDF via fulgur, with deterministic metadata. Re-export `fulgur::inspect` for later tests.

**Files:** Create `src/render.rs`; modify `src/lib.rs`; create `tests/render.rs`

**Acceptance Criteria:**
- [ ] A 2-fragment doc renders to a 2-page PDF at the Move's page size (±1 pt).
- [ ] Rendering the same fragments twice yields **byte-identical** PDFs.

**Verify:** `nix develop -c cargo test --test render` → all pass

**Steps:**

- [ ] **Step 1: Write the failing test** `tests/render.rs`

```rust
use lopdf::Document;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::render_pdf;
use rmbujo::theme::load_theme;
use rmbujo::templates::DotGrid;
use askama::Template;

fn tmp(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-{name}-{n}.pdf"));
    p
}

fn fragments() -> Vec<String> {
    vec![DotGrid.render().unwrap(), DotGrid.render().unwrap()]
}

#[test]
fn page_count_and_size() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let out = tmp("size");
    render_pdf(&dev, &grid, &theme, &fragments(), &out).unwrap();

    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
    let page_id = *doc.get_pages().get(&1).unwrap();
    let mb = doc.get_object(page_id).unwrap().as_dict().unwrap()
        .get(b"MediaBox").unwrap().as_array().unwrap();
    let w = mb[2].as_float().unwrap();
    let h = mb[3].as_float().unwrap();
    assert!((w - dev.width_pt()).abs() < 1.0, "width {w}");
    assert!((h - dev.height_pt()).abs() < 1.0, "height {h}");
}

#[test]
fn deterministic_bytes() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let a = tmp("a");
    let b = tmp("b");
    render_pdf(&dev, &grid, &theme, &fragments(), &a).unwrap();
    render_pdf(&dev, &grid, &theme, &fragments(), &b).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}
```

- [ ] **Step 2: Run → fail.**

- [ ] **Step 3: Write `src/render.rs`**

```rust
//! Assemble HTML + assets and render to PDF via fulgur (Blitz + krilla).

use std::path::Path;

use askama::Template;
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

use crate::device::Device;
use crate::geometry::GridSpec;
use crate::svg;
use crate::templates::Base;
use crate::theme::{css_vars, Palette};

// Re-export inspection for layout tests (Task 10) without a separate dev-dep.
pub use fulgur::inspect::{inspect, InspectResult, TextItem};

const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/DejaVuSerif.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/DejaVuSerif-Bold.ttf");
const FONT_FAMILY: &str = "DejaVu Serif";

fn color<'a>(theme: &'a Palette, key: &str, fallback: &'a str) -> &'a str {
    theme.get(key).map(|s| s.as_str()).unwrap_or(fallback)
}

pub fn build_css(device: &Device, grid: &GridSpec, theme: &Palette) -> String {
    let w = device.width_pt();
    let h = device.height_pt();
    let m = grid.margin_pt;
    let sp = grid.spacing_pt;
    format!(
        "{vars}\n\
@page {{ size: {w}pt {h}pt; margin: 0; }}\n\
* {{ box-sizing: border-box; margin: 0; padding: 0; }}\n\
html, body {{ margin: 0; padding: 0; }}\n\
body {{ font-family: \"{family}\", serif; color: #1a1a1a; }}\n\
.page {{ position: relative; width: {w}pt; height: {h}pt; padding: {m}pt; overflow: hidden; background: #fff; break-after: page; }}\n\
.page:last-child {{ break-after: auto; }}\n\
.dotgrid {{ position: absolute; inset: 0; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; background-position: {m}pt {m}pt; }}\n\
.dotgrid--below {{ top: 24pt; }}\n\
.h-month {{ color: var(--navy); font-size: 16pt; font-weight: bold; margin-bottom: 6pt; }}\n\
.h-section {{ color: var(--navy); font-size: 14pt; font-weight: bold; }}\n\
.daylist {{ display: flex; flex-direction: column; height: calc(100% - 26pt); margin-top: 4pt; }}\n\
.day {{ flex: 1 1 0; display: flex; align-items: center; gap: 8pt; min-height: 0; border-bottom: 0.25pt solid #eeeeee; }}\n\
.day.weekstart {{ border-top: 0.6pt solid var(--rule); }}\n\
.day .num {{ width: 16pt; text-align: right; font-weight: bold; }}\n\
.day .wd {{ color: var(--navy); font-size: 8pt; width: 30pt; }}\n\
.day .gutter {{ flex: 1; }}\n\
.cover {{ position: absolute; inset: 0; display: flex; flex-direction: column; justify-content: flex-end; padding: {m}pt; color: #fff; background-image: url(cover.svg); background-size: 100% 100%; background-repeat: no-repeat; }}\n\
.cover .year {{ font-size: 9pt; letter-spacing: 3px; }}\n\
.cover .title {{ font-size: 24pt; font-weight: bold; }}\n\
.cover .title-blank {{ border-bottom: 1pt solid rgba(255,255,255,0.6); width: 70%; height: 22pt; }}\n\
.fl-block {{ position: relative; height: 33.33%; border-bottom: 0.6pt solid var(--rule); padding-top: 4pt; background-image: url(dot.svg); background-repeat: repeat; background-size: {sp}pt {sp}pt; }}\n\
.fl-block .h-month {{ font-size: 12pt; }}\n\
.legend {{ font-size: 9pt; line-height: 1.8; }}\n\
.legend .sym {{ display: inline-block; width: 16pt; font-weight: bold; color: var(--navy); }}\n\
.pill {{ display: inline-block; padding: 0 6pt; border-radius: 8pt; color: #fff; background: var(--brick); font-size: 7pt; }}\n",
        vars = css_vars(theme), w = w, h = h, m = m, sp = sp, family = FONT_FAMILY,
    )
}

pub fn render_pdf(
    device: &Device,
    grid: &GridSpec,
    theme: &Palette,
    fragments: &[String],
    out_path: &Path,
) -> anyhow::Result<()> {
    let css = build_css(device, grid, theme);
    let html = Base { css: &css, pages: fragments }.render()?;

    let mut assets = AssetBundle::new();
    assets.add_image("dot.svg", svg::dot_tile_svg(grid.spacing_pt, color(theme, "dot", "#CFCDC4")).into_bytes());
    assets.add_image(
        "cover.svg",
        svg::cover_svg(device.width_pt(), device.height_pt(), color(theme, "navy", "#1B365D"), color(theme, "cover_to", "#0F2444")).into_bytes(),
    );
    assets.add_font_bytes(FONT_REGULAR.to_vec())?;
    assets.add_font_bytes(FONT_BOLD.to_vec())?;

    let engine = Engine::builder()
        .page_size(PageSize { width: device.width_pt(), height: device.height_pt() })
        .margin(Margin::uniform(0.0))
        .assets(assets)
        .producer("rmbujo")
        .creator("rmbujo")
        .creation_date("D:20000101000000Z")
        .build();
    engine.render_html_to_file(&html, out_path)?;
    Ok(())
}
```

- [ ] **Step 4: Add `pub mod render;` to `src/lib.rs`.**

- [ ] **Step 5: Run → pass.** If text fails to render with the embedded font, confirm the family name with `nix develop -c fc-scan --format '%{family}\n' assets/fonts/DejaVuSerif.ttf` and set `FONT_FAMILY` to match. If `deterministic_bytes` fails, check no remaining nondeterministic field is emitted by fulgur (it should be byte-stable with fixed `creation_date`).

- [ ] **Step 6: Commit**

```bash
git add src/render.rs src/lib.rs tests/render.rs
git commit -m "Add fulgur renderer with SVG assets, embedded font, deterministic metadata"
```

```json:metadata
{"files": ["src/render.rs", "src/lib.rs", "tests/render.rs"], "verifyCommand": "nix develop -c cargo test --test render", "acceptanceCriteria": ["2-page PDF at move size", "byte-identical on repeat render"], "requiresUserVerification": false}
```

---

### Task 9: Notebook builders (`src/notebooks/`)

**Goal:** Build each notebook's fragments and render one PDF with the correct page count.

**Files:** Create `src/notebooks/{mod,month,future_log,collection,reference}.rs`; modify `src/lib.rs`; create `tests/notebooks.rs`

**Acceptance Criteria:**
- [ ] Month = `2 + daily_pages` pages; future-log = 5; collection = `1 + collection_pages`; reference = 3.

**Verify:** `nix develop -c cargo test --test notebooks` → all pass

**Steps:**

- [ ] **Step 1: Write `src/notebooks/mod.rs`**

```rust
//! Notebook builders: assemble page fragments and render one PDF each.

pub mod collection;
pub mod future_log;
pub mod month;
pub mod reference;

use std::path::Path;

use crate::config::Config;
use crate::{device, geometry, render, theme};

fn render_notebook(config: &Config, fragments: &[String], out_path: &Path) -> anyhow::Result<()> {
    let dev = device::get_device(&config.device)?;
    let grid = geometry::default_grid(&dev);
    let th = theme::load_theme(&config.theme)?;
    render::render_pdf(&dev, &grid, &th, fragments, out_path)
}
```

- [ ] **Step 2: Write `src/notebooks/month.rs`**

```rust
use std::path::Path;

use askama::Template;

use crate::calendar::build_month;
use crate::config::Config;
use crate::templates::{DayView, DotGrid, MonthIndex, Tasks};

pub fn build_month_pdf(config: &Config, month: u32, out_path: &Path) -> anyhow::Result<()> {
    let m = build_month(config.year, month, &config.week_start)?;
    let days: Vec<DayView> = m.days.iter()
        .map(|d| DayView { day: d.day, weekday: d.weekday, week_start: d.week_start })
        .collect();
    let mut fragments = vec![
        MonthIndex { month_name: m.name, year: config.year, days: &days }.render()?,
        Tasks.render()?,
    ];
    for _ in 0..config.daily_pages {
        fragments.push(DotGrid.render()?);
    }
    super::render_notebook(config, &fragments, out_path)
}
```

- [ ] **Step 3: Write `src/notebooks/future_log.rs`**

```rust
use std::path::Path;

use askama::Template;

use crate::calendar::MONTH_NAMES;
use crate::config::Config;
use crate::templates::{Cover, FutureLog};

pub fn build_future_log_pdf(config: &Config, out_path: &Path) -> anyhow::Result<()> {
    let names: &[&str] = &MONTH_NAMES[1..]; // 12 month names
    let mut fragments = vec![Cover { year: config.year, title: "Future Log", blank_title: false }.render()?];
    for chunk in names.chunks(3) {
        fragments.push(FutureLog { months: chunk }.render()?);
    }
    super::render_notebook(config, &fragments, out_path)
}
```

- [ ] **Step 4: Write `src/notebooks/collection.rs`**

```rust
use std::path::Path;

use askama::Template;

use crate::config::Config;
use crate::templates::{Cover, DotGrid};

pub fn build_collection_pdf(config: &Config, out_path: &Path) -> anyhow::Result<()> {
    let mut fragments = vec![Cover { year: config.year, title: "", blank_title: true }.render()?];
    for _ in 0..config.collection_pages {
        fragments.push(DotGrid.render()?);
    }
    super::render_notebook(config, &fragments, out_path)
}
```

- [ ] **Step 5: Write `src/notebooks/reference.rs`**

```rust
use std::path::Path;

use askama::Template;

use crate::config::Config;
use crate::templates::{Cover, Reference};

pub fn build_reference_pdf(config: &Config, out_path: &Path) -> anyhow::Result<()> {
    let fragments = vec![
        Cover { year: config.year, title: "Reference", blank_title: false }.render()?,
        Reference.render()?,
    ];
    super::render_notebook(config, &fragments, out_path)
}
```

- [ ] **Step 6: Add `pub mod notebooks;` to `src/lib.rs`.**

- [ ] **Step 7: Write the test** `tests/notebooks.rs`

```rust
use lopdf::Document;
use rmbujo::config::Config;
use rmbujo::notebooks::{collection, future_log, month, reference};

fn tmp() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-nb-{n}.pdf"));
    p
}

fn pages(p: &std::path::Path) -> usize {
    Document::load(p).unwrap().get_pages().len()
}

#[test]
fn month_pages() {
    let cfg = Config { daily_pages: 5, ..Config::new(2026) };
    let out = tmp();
    month::build_month_pdf(&cfg, 5, &out).unwrap();
    assert_eq!(pages(&out), 2 + 5);
}

#[test]
fn future_log_pages() {
    let out = tmp();
    future_log::build_future_log_pdf(&Config::new(2026), &out).unwrap();
    assert_eq!(pages(&out), 5);
}

#[test]
fn collection_pages() {
    let cfg = Config { collection_pages: 4, ..Config::new(2026) };
    let out = tmp();
    collection::build_collection_pdf(&cfg, &out).unwrap();
    assert_eq!(pages(&out), 1 + 4);
}

#[test]
fn reference_pages() {
    let out = tmp();
    reference::build_reference_pdf(&Config::new(2026), &out).unwrap();
    assert_eq!(pages(&out), 3);
}
```

- [ ] **Step 8: Run → pass.**

- [ ] **Step 9: Commit**

```bash
git add src/notebooks/ src/lib.rs tests/notebooks.rs
git commit -m "Add notebook builders"
```

```json:metadata
{"files": ["src/notebooks/mod.rs", "src/notebooks/month.rs", "src/notebooks/future_log.rs", "src/notebooks/collection.rs", "src/notebooks/reference.rs", "tests/notebooks.rs"], "verifyCommand": "nix develop -c cargo test --test notebooks", "acceptanceCriteria": ["month=2+daily", "future-log=5", "collection=1+N", "reference=3"], "requiresUserVerification": false}
```

---

### Task 10: Layout / overlap inspection tests (`tests/layout.rs`)

**Goal:** Geometrically assert that text never overlaps and never overflows the page — catching the font-overlap bug seen during theme prototyping, on the first render.

**Files:** Create `tests/layout.rs`

**Acceptance Criteria:**
- [ ] For the month, future-log, and reference notebooks, no two `TextItem` boxes on a page intersect (with tolerance), and every text box lies within the page bounds.
- [ ] Key text (e.g. "May", "2026", "18", "Mon") is present in the inspected PDF — confirming text rendered into the PDF, not just into the HTML.

**Verify:** `nix develop -c cargo test --test layout` → all pass

**Steps:**

- [ ] **Step 1: Write the test** `tests/layout.rs`

```rust
use rmbujo::config::Config;
use rmbujo::device::get_device;
use rmbujo::notebooks::{future_log, month, reference};
use rmbujo::render::{inspect, TextItem};

const TOL: f32 = 0.5; // pt — accommodates inspect()'s estimated text widths

fn tmp(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-layout-{tag}-{n}.pdf"));
    p
}

fn overlaps(a: &TextItem, b: &TextItem) -> bool {
    if a.page != b.page {
        return false;
    }
    let x_overlap = a.x < b.x + b.width - TOL && b.x < a.x + a.width - TOL;
    let y_overlap = a.y < b.y + b.height - TOL && b.y < a.y + a.height - TOL;
    x_overlap && y_overlap
}

fn assert_no_overlap_and_in_bounds(pdf: &std::path::Path) {
    let dev = get_device("paper-pro-move").unwrap();
    let (w, h) = (dev.width_pt(), dev.height_pt());
    let result = inspect(pdf).unwrap();

    // In-bounds: every text box within [0,w] x [0,h] (with tolerance).
    for t in &result.text_items {
        assert!(
            t.x >= -TOL && t.y >= -TOL && t.x + t.width <= w + TOL && t.y + t.height <= h + TOL,
            "text {:?} out of page bounds: x={} y={} w={} h={} (page {}x{})",
            t.text, t.x, t.y, t.width, t.height, w, h,
        );
    }
    // No-overlap: pairwise on the same page.
    let items = &result.text_items;
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            assert!(
                !overlaps(&items[i], &items[j]),
                "text overlap on page {}: {:?} <-> {:?}",
                items[i].page, items[i].text, items[j].text,
            );
        }
    }
}

fn assert_text_present(pdf: &std::path::Path, needles: &[&str]) {
    let result = inspect(pdf).unwrap();
    let all: String = result.text_items.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ");
    for n in needles {
        assert!(all.contains(n), "expected text {n:?} in rendered PDF, got: {all:?}");
    }
}

#[test]
fn month_layout_clean() {
    let cfg = Config { daily_pages: 1, ..Config::new(2026) };
    let out = tmp("month");
    month::build_month_pdf(&cfg, 5, &out).unwrap();
    assert_no_overlap_and_in_bounds(&out);
    // Text actually rendered into the PDF (not just present in the HTML).
    assert_text_present(&out, &["May", "2026", "18", "Mon"]);
}

#[test]
fn future_log_layout_clean() {
    let out = tmp("fl");
    future_log::build_future_log_pdf(&Config::new(2026), &out).unwrap();
    assert_no_overlap_and_in_bounds(&out);
}

#[test]
fn reference_layout_clean() {
    let out = tmp("ref");
    reference::build_reference_pdf(&Config::new(2026), &out).unwrap();
    assert_no_overlap_and_in_bounds(&out);
}
```

- [ ] **Step 2: Run.** `nix develop -c cargo test --test layout`
Expected: pass. If a real overlap is found (e.g. a weekday column too narrow so text spills into the gutter), fix the CSS in `src/render.rs::build_css` (widen the column / reduce font-size) and re-run — this test is doing its job.

- [ ] **Step 3: Commit**

```bash
git add tests/layout.rs
git commit -m "Add geometric layout overlap/bounds tests"
```

```json:metadata
{"files": ["tests/layout.rs"], "verifyCommand": "nix develop -c cargo test --test layout", "acceptanceCriteria": ["no text-box overlap per page", "all text within page bounds", "key text present in rendered PDF"], "requiresUserVerification": false}
```

---

### Task 11: Year orchestrator + deploy seam (`src/generate.rs`, `src/deploy/`)

**Goal:** Build a full year of PDFs with correct filenames behind a no-op `Deployer`.

**Files:** Create `src/generate.rs`, `src/deploy/{mod,local}.rs`; modify `src/lib.rs`; create `tests/generate.rs`

**Acceptance Criteria:**
- [ ] `generate_year` writes 15 PDFs with the exact spec filenames.
- [ ] `get_deployer` returns a no-op for `"none"` and errors for unknown backends.

**Verify:** `nix develop -c cargo test --test generate` → all pass

**Steps:**

- [ ] **Step 1: Write `src/deploy/mod.rs`**

```rust
//! Deploy seam (Phase 2 fills in the rmapi backend).

pub mod local;

use std::path::PathBuf;

use crate::config::Config;

pub trait Deployer {
    fn deploy(&self, paths: &[PathBuf]) -> anyhow::Result<()>;
    fn refresh(&self, paths: &[PathBuf]) -> anyhow::Result<()>;
}

pub fn get_deployer(config: &Config) -> anyhow::Result<Box<dyn Deployer>> {
    match config.deploy.backend.as_str() {
        "none" => Ok(Box::new(local::LocalDeployer)),
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
}
```

- [ ] **Step 2: Write `src/deploy/local.rs`**

```rust
//! Local backend "none": PDFs are already on disk; deploy/refresh are no-ops.

use std::path::PathBuf;

use super::Deployer;

pub struct LocalDeployer;

impl Deployer for LocalDeployer {
    fn deploy(&self, _paths: &[PathBuf]) -> anyhow::Result<()> {
        Ok(())
    }
    fn refresh(&self, _paths: &[PathBuf]) -> anyhow::Result<()> {
        Ok(())
    }
}
```

- [ ] **Step 3: Write `src/generate.rs`**

```rust
//! Orchestrate building a whole year of notebook PDFs.

use std::path::{Path, PathBuf};

use crate::calendar::MONTH_NAMES;
use crate::config::Config;
use crate::notebooks::{collection, future_log, month, reference};

pub fn generate_year(config: &Config, out_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)?;
    let y = config.year;
    let mut paths = Vec::new();

    let fl = out_dir.join(format!("{y} Future Log.pdf"));
    future_log::build_future_log_pdf(config, &fl)?;
    paths.push(fl);

    for mo in 1..=12u32 {
        let p = out_dir.join(format!("{y}.{mo:02} {name}.pdf", name = MONTH_NAMES[mo as usize]));
        month::build_month_pdf(config, mo, &p)?;
        paths.push(p);
    }

    let col = out_dir.join(format!("{y} Collection Template.pdf"));
    collection::build_collection_pdf(config, &col)?;
    paths.push(col);

    let r = out_dir.join(format!("{y} Reference.pdf"));
    reference::build_reference_pdf(config, &r)?;
    paths.push(r);

    Ok(paths)
}
```

- [ ] **Step 4: Add `pub mod deploy;` and `pub mod generate;` to `src/lib.rs`.**

- [ ] **Step 5: Write the test** `tests/generate.rs`

```rust
use rmbujo::config::{Config, DeployConfig};
use rmbujo::deploy::{get_deployer, local::LocalDeployer};
use rmbujo::generate::generate_year;

fn tmp_dir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-gen-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn writes_15_named_pdfs() {
    let cfg = Config { daily_pages: 1, collection_pages: 1, ..Config::new(2026) };
    let dir = tmp_dir();
    let paths = generate_year(&cfg, &dir).unwrap();
    assert_eq!(paths.len(), 15);
    for f in [
        "2026 Future Log.pdf",
        "2026.05 May.pdf",
        "2026 Collection Template.pdf",
        "2026 Reference.pdf",
    ] {
        assert!(dir.join(f).exists(), "missing {f}");
    }
}

#[test]
fn deployer_none_ok_unknown_errs() {
    let _: LocalDeployer = LocalDeployer; // type exists
    assert!(get_deployer(&Config::new(2026)).is_ok());
    let bad = Config { deploy: DeployConfig { backend: "rmapi".into(), target_folder: String::new() }, ..Config::new(2026) };
    assert!(get_deployer(&bad).is_err());
}
```

- [ ] **Step 6: Run → pass.**

- [ ] **Step 7: Commit**

```bash
git add src/generate.rs src/deploy/ src/lib.rs tests/generate.rs
git commit -m "Add year orchestrator and no-op deploy seam"
```

```json:metadata
{"files": ["src/generate.rs", "src/deploy/mod.rs", "src/deploy/local.rs", "tests/generate.rs"], "verifyCommand": "nix develop -c cargo test --test generate", "acceptanceCriteria": ["15 named PDFs", "deployer none ok, unknown errs"], "requiresUserVerification": false}
```

---

### Task 12: CLI + wizard (`src/cli.rs`, `src/wizard.rs`, `src/main.rs`)

**Goal:** `rmbujo new` runs the wizard (creates year folder + `rmbujo.toml`, builds); `rmbujo <config.toml>` regenerates.

**Files:** Create `src/cli.rs`, `src/wizard.rs`; modify `src/lib.rs`, `src/main.rs`; create `tests/cli.rs`

**Acceptance Criteria:**
- [ ] `wizard::assemble` returns the expected `Config`, out dir `<base>/<year>`, and config path `<out>/rmbujo.toml`.
- [ ] `cli::run(["rmbujo", <path>])` regenerates the year's PDFs into the config's directory.

**Verify:** `nix develop -c cargo test --test cli` → all pass

**Steps:**

- [ ] **Step 1: Write `src/wizard.rs`**

```rust
//! Interactive "new year" wizard. `assemble` is pure (testable); `run_wizard` prompts.

use std::path::PathBuf;

use chrono::Datelike;

use crate::config::{Config, DeployConfig};

pub struct Answers {
    pub year: i32,
    pub base: String,
    pub device: String,
    pub week_start: String,
    pub daily_pages: u32,
    pub collection_pages: u32,
    pub theme: String,
}

/// Build a Config + paths from gathered answers (no I/O).
pub fn assemble(a: Answers) -> (Config, PathBuf, PathBuf) {
    let config = Config {
        year: a.year,
        device: a.device,
        week_start: a.week_start,
        daily_pages: a.daily_pages,
        collection_pages: a.collection_pages,
        theme: a.theme,
        ics: Vec::new(),
        deploy: DeployConfig { backend: "none".into(), target_folder: format!("/{}", a.year) },
    };
    let out_dir = PathBuf::from(a.base).join(a.year.to_string());
    let config_path = out_dir.join("rmbujo.toml");
    (config, out_dir, config_path)
}

/// Prompt the user (dialoguer), create the out dir, and return Config + paths.
pub fn run_wizard() -> anyhow::Result<(Config, PathBuf, PathBuf)> {
    use dialoguer::Input;

    let year: i32 = Input::new()
        .with_prompt("Year")
        .default(chrono::Local::now().year())
        .interact_text()?;
    let base: String = Input::new().with_prompt("Base directory").default(".".into()).interact_text()?;
    let device: String = Input::new().with_prompt("Device").default("paper-pro-move".into()).interact_text()?;
    let week_start: String = Input::new().with_prompt("Week start (sun|mon)").default("sun".into()).interact_text()?;
    let daily_pages: u32 = Input::new().with_prompt("Daily pages per month").default(60).interact_text()?;
    let collection_pages: u32 = Input::new().with_prompt("Collection pages").default(20).interact_text()?;
    let theme: String = Input::new().with_prompt("Theme").default("library".into()).interact_text()?;

    let (config, out_dir, config_path) = assemble(Answers {
        year, base, device, week_start, daily_pages, collection_pages, theme,
    });
    std::fs::create_dir_all(&out_dir)?;
    Ok((config, out_dir, config_path))
}
```

- [ ] **Step 2: Write `src/cli.rs`**

```rust
//! rmbujo CLI: `new` wizard, or regenerate from a config path.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::{config, deploy, generate, wizard};

#[derive(Parser)]
#[command(name = "rmbujo", version, about = "Dot-grid bullet journal PDF generator for reMarkable", args_conflicts_with_subcommands = true)]
struct Cli {
    /// Path to an existing rmbujo.toml to regenerate.
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new year interactively.
    New,
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    let cli = Cli::try_parse_from(args)?;
    match (cli.command, cli.config) {
        (Some(Command::New), _) => {
            let (config, out_dir, config_path) = wizard::run_wizard()?;
            config::dump(&config, &config_path)?;
            let paths = generate::generate_year(&config, &out_dir)?;
            deploy::get_deployer(&config)?.deploy(&paths)?;
            println!("Wrote {} PDFs to {}", paths.len(), out_dir.display());
            Ok(())
        }
        (None, Some(path)) => {
            let config = config::load(&path)?;
            let out_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let paths = generate::generate_year(&config, &out_dir)?;
            deploy::get_deployer(&config)?.refresh(&paths)?;
            println!("Regenerated {} PDFs in {}", paths.len(), out_dir.display());
            Ok(())
        }
        (None, None) => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

pub fn main() -> anyhow::Result<()> {
    run(std::env::args().collect())
}
```

- [ ] **Step 3: Update `src/main.rs`**

```rust
fn main() -> anyhow::Result<()> {
    rmbujo::cli::main()
}
```

- [ ] **Step 4: Add `pub mod cli;` and `pub mod wizard;` to `src/lib.rs`.**

- [ ] **Step 5: Write the test** `tests/cli.rs`

```rust
use rmbujo::cli::run;
use rmbujo::config::{self, Config};
use rmbujo::wizard::{assemble, Answers};

fn tmp_dir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-cli-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

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
        theme: "library".into(),
    });
    assert_eq!(config.year, 2026);
    assert_eq!(config.daily_pages, 3);
    assert_eq!(out_dir, base.join("2026"));
    assert_eq!(config_path, base.join("2026").join("rmbujo.toml"));
}

#[test]
fn regenerate_from_config() {
    let dir = tmp_dir().join("2026");
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config { daily_pages: 1, collection_pages: 1, ..Config::new(2026) };
    config::dump(&cfg, &dir.join("rmbujo.toml")).unwrap();

    run(vec!["rmbujo".into(), dir.join("rmbujo.toml").to_string_lossy().into_owned()]).unwrap();

    assert!(dir.join("2026.05 May.pdf").exists());
    assert!(dir.join("2026 Reference.pdf").exists());
}
```

- [ ] **Step 6: Run → pass.**

- [ ] **Step 7: Commit**

```bash
git add src/cli.rs src/wizard.rs src/main.rs src/lib.rs tests/cli.rs
git commit -m "Add CLI dispatch and interactive wizard"
```

```json:metadata
{"files": ["src/cli.rs", "src/wizard.rs", "src/main.rs", "tests/cli.rs"], "verifyCommand": "nix develop -c cargo test --test cli", "acceptanceCriteria": ["wizard assemble returns Config/out_dir/config_path", "regenerate from config writes PDFs"], "requiresUserVerification": false}
```

---

### Task 13: Visual regression goldens (`tests/visual.rs`)

**Goal:** Rasterize each page type and compare to committed golden PNGs within tolerance; `RMBUJO_UPDATE_GOLDENS=1` (re)writes goldens.

**Files:** Create `tests/visual.rs`, `tests/goldens/` (committed PNGs)

**Acceptance Criteria:**
- [ ] First run with `RMBUJO_UPDATE_GOLDENS=1` writes a golden PNG per page type.
- [ ] A subsequent normal run passes with a diff ratio below tolerance for each.

**Verify:** `nix develop -c env RMBUJO_UPDATE_GOLDENS=1 cargo test --test visual` then `nix develop -c cargo test --test visual` → pass

**Steps:**

- [ ] **Step 1: Write the test** `tests/visual.rs`

```rust
use std::path::{Path, PathBuf};
use std::process::Command;

use askama::Template;
use image::GenericImageView;
use rmbujo::calendar::build_month;
use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;
use rmbujo::render::render_pdf;
use rmbujo::templates::{Cover, DayView, DotGrid, FutureLog, MonthIndex, Reference, Tasks};
use rmbujo::theme::load_theme;

const TOLERANCE: f64 = 0.01; // max fraction of differing pixels

fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens")
}

fn tmp(tag: &str, ext: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-vis-{tag}-{n}.{ext}"));
    p
}

fn fragment_pages() -> Vec<(&'static str, String)> {
    let m = build_month(2026, 5, "sun").unwrap();
    let days: Vec<DayView> = m.days.iter()
        .map(|d| DayView { day: d.day, weekday: d.weekday, week_start: d.week_start })
        .collect();
    vec![
        ("cover", Cover { year: 2026, title: "Future Log", blank_title: false }.render().unwrap()),
        ("cover_blank", Cover { year: 2026, title: "", blank_title: true }.render().unwrap()),
        ("dotgrid", DotGrid.render().unwrap()),
        ("tasks", Tasks.render().unwrap()),
        ("month_index", MonthIndex { month_name: "May", year: 2026, days: &days }.render().unwrap()),
        ("future_log", FutureLog { months: &["January", "February", "March"] }.render().unwrap()),
        ("reference", Reference.render().unwrap()),
    ]
}

/// Render one fragment to a single-page PDF, then rasterize page 1 to PNG via pdftoppm.
fn render_png(fragment: &str, png: &Path) {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let pdf = tmp("page", "pdf");
    render_pdf(&dev, &grid, &theme, &[fragment.to_string()], &pdf).unwrap();

    // pdftoppm writes "<prefix>-1.png" for a single page; use prefix without extension.
    let prefix = png.with_extension("");
    let status = Command::new("pdftoppm")
        .args(["-png", "-r", "150", "-singlefile", pdf.to_str().unwrap(), prefix.to_str().unwrap()])
        .status()
        .expect("pdftoppm");
    assert!(status.success(), "pdftoppm failed");
}

fn diff_ratio(a: &Path, b: &Path) -> f64 {
    let ia = image::open(a).unwrap().to_rgb8();
    let ib = image::open(b).unwrap().to_rgb8();
    if ia.dimensions() != ib.dimensions() {
        return 1.0;
    }
    let total = (ia.width() * ia.height()) as f64;
    let mut diff = 0u64;
    for (pa, pb) in ia.pixels().zip(ib.pixels()) {
        if pa != pb {
            diff += 1;
        }
    }
    diff as f64 / total
}

#[test]
fn visual_regression() {
    let update = std::env::var("RMBUJO_UPDATE_GOLDENS").is_ok();
    std::fs::create_dir_all(goldens_dir()).unwrap();

    for (name, fragment) in fragment_pages() {
        let shot = tmp(name, "png");
        render_png(&fragment, &shot);
        let golden = goldens_dir().join(format!("{name}.png"));
        if update {
            std::fs::copy(&shot, &golden).unwrap();
            continue;
        }
        assert!(golden.exists(), "missing golden {name}; run `make update-goldens`");
        let ratio = diff_ratio(&shot, &golden);
        assert!(ratio < TOLERANCE, "{name} differs by {ratio:.4} (> {TOLERANCE})");
    }
}
```

- [ ] **Step 2: Generate goldens.**

Run: `nix develop -c env RMBUJO_UPDATE_GOLDENS=1 cargo test --test visual`
Expected: pass; `tests/goldens/*.png` created (7 files).

- [ ] **Step 3: Verify comparison.**

Run: `nix develop -c cargo test --test visual`
Expected: pass.

- [ ] **Step 4: Commit (including goldens)**

```bash
git add tests/visual.rs tests/goldens/
git commit -m "Add visual-regression tests with golden images"
```

```json:metadata
{"files": ["tests/visual.rs", "tests/goldens/"], "verifyCommand": "nix develop -c cargo test --test visual", "acceptanceCriteria": ["goldens generated per page type", "comparison passes within tolerance"], "requiresUserVerification": false}
```

---

### Task 14: README + full suite green

**Goal:** Document usage and confirm the entire suite passes together.

**Files:** Modify `README.md`

**Acceptance Criteria:**
- [ ] README documents the Nix/direnv setup and both invocation modes.
- [ ] `nix develop -c cargo test` passes for the whole suite; `cargo clippy -- -D warnings` is clean.

**Verify:** `nix develop -c cargo test` → all pass

**Steps:**

- [ ] **Step 1: Write `README.md`**

```markdown
# rmbujo

Dot-grid bullet-journal PDF generator for reMarkable devices (Paper Pro Move / Paper Pro),
written in Rust. Renders askama HTML/CSS via fulgur (Blitz + krilla) — no headless browser.

## Setup

Dependencies are managed with Nix. With direnv:

    direnv allow        # loads the flake dev shell automatically

Or manually: `nix develop`.

## Usage

Create a new year (interactive wizard — creates `<base>/<year>/` and its `rmbujo.toml`,
then generates the PDFs):

    rmbujo new

Regenerate an existing year from its config:

    rmbujo path/to/2026/rmbujo.toml

## Output

A flat folder per year, one PDF per notebook: `2026 Future Log.pdf`,
`2026.01 January.pdf` … `2026.12 December.pdf`, `2026 Collection Template.pdf`,
`2026 Reference.pdf`.

## Development

    make test             # full suite in the Nix shell
    make update-goldens   # regenerate visual-regression golden images
    make clippy           # lints
    make build            # nix build the rmbujo package

ICS calendar feeds (incl. holidays) and reMarkable cloud sync (via rmapi) are Phase 2;
see `docs/superpowers/specs/2026-05-20-rmbujo-design.md`.
```

- [ ] **Step 2: Run the full suite + lints.**

Run: `nix develop -c cargo test`
Run: `nix develop -c cargo clippy -- -D warnings`
Expected: all green.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "Add README and confirm full suite green"
```

```json:metadata
{"files": ["README.md"], "verifyCommand": "nix develop -c cargo test", "acceptanceCriteria": ["README documents setup + usage", "full suite + clippy pass"], "requiresUserVerification": false}
```

---

## Dependencies Between Tasks

- Task 0 blocks everything.
- Tasks 1–6 are independent (after 0). [device, calendar, config, theme, geometry, svg]
- Task 7 (templates) depends on 2 (calendar, used in its test).
- Task 8 (render) depends on 1, 5, 6, 7 (device, geometry, svg, templates).
- Task 9 (notebooks) depends on 8, 3, 2 (render, config, calendar).
- Task 10 (layout tests) depends on 9.
- Task 11 (generate + deploy) depends on 9, 3.
- Task 12 (cli + wizard) depends on 11, 3.
- Task 13 (visual) depends on 8, 7.
- Task 14 depends on all.
