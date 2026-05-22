//! Per-year config: serde structs + TOML load/dump.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IcsFeed {
    pub name: String,
    pub url: String,
    /// Swatch color (a theme color name). Omit it to auto-assign a distinct color
    /// from the calendar palette by feed order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Calendar swatch palette: theme color names auto-assigned to feeds in order
/// (feed 1 -> cal1, ...). Up to 10 distinct hues; cycles after that.
pub const CAL_PALETTE: [&str; 10] = [
    "cal1", "cal2", "cal3", "cal4", "cal5", "cal6", "cal7", "cal8", "cal9", "cal10",
];

impl IcsFeed {
    /// The swatch color for this feed: its explicit `color`, else an auto-assigned
    /// palette slot by `index` (0-based feed order).
    pub fn color_for(&self, index: usize) -> String {
        self.color
            .clone()
            .unwrap_or_else(|| CAL_PALETTE[index % CAL_PALETTE.len()].to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeployConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub base_folder: String,
}
fn default_backend() -> String {
    "none".into()
}
impl Default for DeployConfig {
    fn default() -> Self {
        Self {
            backend: "none".into(),
            base_folder: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub year: i32,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_week_start")]
    pub week_start: String,
    #[serde(default = "default_collection")]
    pub collection_pages: u32,
    #[serde(default = "default_spacing")]
    pub spacing_mm: f32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_pages_per_day")]
    pub pages_per_day: u32,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub ics: Vec<IcsFeed>,
    #[serde(default)]
    pub deploy: DeployConfig,
}
fn default_device() -> String {
    "paper-pro-move".into()
}
fn default_week_start() -> String {
    "sun".into()
}
fn default_collection() -> u32 {
    20
}
fn default_spacing() -> f32 {
    crate::geometry::DEFAULT_SPACING_MM
}
fn default_theme() -> String {
    "library".into()
}
fn default_pages_per_day() -> u32 {
    1
}
fn default_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into())
}

impl Config {
    /// A config with the given year and all other fields defaulted.
    pub fn new(year: i32) -> Self {
        Config {
            year,
            device: default_device(),
            week_start: default_week_start(),
            collection_pages: default_collection(),
            spacing_mm: default_spacing(),
            theme: default_theme(),
            pages_per_day: default_pages_per_day(),
            timezone: default_timezone(),
            ics: Vec::new(),
            deploy: DeployConfig::default(),
        }
    }

    /// Validate the device, theme, and week_start up front, before any output is
    /// written, so bad input fails fast with a clear message (spec: validate at
    /// parse time) rather than partway through rendering.
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::device::get_device(&self.device)?;
        let theme = crate::theme::load_theme(&self.theme)?;
        match self.week_start.as_str() {
            "sun" | "mon" => {}
            other => anyhow::bail!("week_start must be 'sun' or 'mon', got {other:?}"),
        }
        match self.deploy.backend.as_str() {
            "none" | "rmapi" => {}
            other => anyhow::bail!("deploy.backend must be 'none' or 'rmapi', got {other:?}"),
        }
        // Keep dot pitch in a sane, writable range so a typo can't produce a
        // single dot or a solid field. 2–10 mm spans far tighter and far looser
        // than any usable grid.
        if !(self.spacing_mm.is_finite() && (2.0..=10.0).contains(&self.spacing_mm)) {
            anyhow::bail!(
                "spacing_mm must be between 2.0 and 10.0, got {}",
                self.spacing_mm
            );
        }
        if self.pages_per_day == 0 {
            anyhow::bail!("pages_per_day must be >= 1");
        }
        if self.timezone.parse::<chrono_tz::Tz>().is_err() {
            anyhow::bail!("unknown timezone: {:?}", self.timezone);
        }
        // An explicit ICS feed color must name a theme color (catch typos); an
        // omitted color is auto-assigned from the palette, which is always valid.
        for feed in &self.ics {
            if let Some(c) = &feed.color {
                if !theme.contains_key(c.as_str()) {
                    let mut names: Vec<&str> = theme.keys().map(String::as_str).collect();
                    names.sort_unstable();
                    anyhow::bail!(
                        "ics feed {:?}: unknown color {c:?}; choose a theme color ({})",
                        feed.name,
                        names.join(", ")
                    );
                }
            }
        }
        Ok(())
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
