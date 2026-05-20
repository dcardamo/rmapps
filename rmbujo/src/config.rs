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
