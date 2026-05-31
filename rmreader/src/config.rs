//! rmreader config: serde structs + TOML load/dump + validate.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadwiseConfig {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryConfig {
    #[serde(default = "default_library_locations")]
    pub locations: Vec<String>,
    #[serde(default = "default_max_items")]
    pub max_items: u32,
}
fn default_library_locations() -> Vec<String> {
    vec!["new".into(), "later".into(), "shortlist".into()]
}
fn default_max_items() -> u32 {
    100
}
impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            locations: default_library_locations(),
            max_items: default_max_items(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_items")]
    pub max_items: u32,
}
fn default_true() -> bool {
    true
}
impl Default for FeedConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_items: default_max_items(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImagesConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}
fn default_timeout_secs() -> u64 {
    8
}
fn default_concurrency() -> usize {
    12
}
impl Default for ImagesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_secs: default_timeout_secs(),
            concurrency: default_concurrency(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentConfig {
    #[serde(default = "default_max_article_bytes")]
    pub max_article_bytes: usize,
}
fn default_max_article_bytes() -> usize {
    80_000
}
impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            max_article_bytes: default_max_article_bytes(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeployConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_reader_folder")]
    pub library_folder: String,
    #[serde(default = "default_reader_folder")]
    pub feed_folder: String,
}
fn default_backend() -> String {
    "none".into()
}
fn default_reader_folder() -> String {
    "/Readwise".into()
}
impl Default for DeployConfig {
    fn default() -> Self {
        Self {
            backend: "none".into(),
            library_folder: default_reader_folder(),
            feed_folder: default_reader_folder(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Cache directory. `None` → resolved at runtime to
    /// `$XDG_CACHE_HOME/rmreader` (else `~/.cache/rmreader`).
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default = "default_expiry_days")]
    pub expiry_days: u64,
}
fn default_expiry_days() -> u64 {
    7
}
impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: None,
            expiry_days: 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    pub readwise: ReadwiseConfig,
    #[serde(default)]
    pub library: LibraryConfig,
    #[serde(default)]
    pub feed: FeedConfig,
    #[serde(default)]
    pub images: ImagesConfig,
    #[serde(default)]
    pub content: ContentConfig,
    #[serde(default)]
    pub deploy: DeployConfig,
    #[serde(default)]
    pub cache: CacheConfig,
}
fn default_device() -> String {
    "paper-pro-move".into()
}
fn default_output_dir() -> String {
    ".".into()
}
fn default_theme() -> String {
    "reader".into()
}

const VALID_LOCATIONS: &[&str] = &["new", "later", "shortlist", "archive", "feed"];

impl Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::device::get_device(&self.device)?;
        crate::theme::load_theme(&self.theme)?;
        if self.readwise.token.trim().is_empty() {
            anyhow::bail!(
                "readwise.token is required (get one at https://readwise.io/access_token)"
            );
        }
        if self.library.locations.is_empty() {
            anyhow::bail!("library.locations must list at least one location");
        }
        for loc in &self.library.locations {
            if !VALID_LOCATIONS.contains(&loc.as_str()) {
                anyhow::bail!("invalid library location {loc:?}; choices: {VALID_LOCATIONS:?}");
            }
        }
        match self.deploy.backend.as_str() {
            "none" => {}
            "rmapi" => {
                if self.deploy.library_folder.trim().is_empty() {
                    anyhow::bail!("deploy.library_folder is required for the rmapi backend");
                }
                if self.feed.enabled && self.deploy.feed_folder.trim().is_empty() {
                    anyhow::bail!(
                        "deploy.feed_folder is required when feed is enabled and backend is rmapi"
                    );
                }
            }
            other => anyhow::bail!("deploy.backend must be 'none' or 'rmapi', got {other:?}"),
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
