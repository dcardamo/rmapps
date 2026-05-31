//! TOML configuration for rmdigest.
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub device: String,
    pub watched_paths: Vec<String>,
    #[serde(default)]
    pub deploy: Deploy,
    #[serde(default)]
    pub output: Output,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Deploy {
    #[serde(default = "default_backend")]
    pub backend: String, // "rmapi" | "none"
}
impl Default for Deploy {
    fn default() -> Self {
        Self {
            backend: default_backend(),
        }
    }
}
fn default_backend() -> String {
    "rmapi".into()
}

#[derive(Debug, Deserialize, Clone)]
pub struct Output {
    #[serde(default = "default_annot")]
    pub annotated_suffix: String,
    #[serde(default = "default_digest")]
    pub digest_suffix: String,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            annotated_suffix: default_annot(),
            digest_suffix: default_digest(),
        }
    }
}
fn default_annot() -> String {
    " — Annotated".into()
}
fn default_digest() -> String {
    " — Digest".into()
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.watched_paths.is_empty() {
            anyhow::bail!("watched_paths must not be empty");
        }
        if !matches!(self.deploy.backend.as_str(), "rmapi" | "none") {
            anyhow::bail!("deploy.backend must be \"rmapi\" or \"none\"");
        }
        if self.output.annotated_suffix == self.output.digest_suffix {
            anyhow::bail!("annotated_suffix and digest_suffix must differ");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_minimal() {
        let toml = r#"
device = "paper-pro-move"
watched_paths = ["/Books", "/Documents"]
"#;
        let c: Config = toml::from_str(toml).unwrap();
        c.validate().unwrap();
        assert_eq!(c.deploy.backend, "rmapi");
        assert_eq!(c.output.annotated_suffix, " — Annotated");
    }
    #[test]
    fn rejects_empty_paths() {
        let c = Config {
            device: "x".into(),
            watched_paths: vec![],
            deploy: Default::default(),
            output: Default::default(),
        };
        assert!(c.validate().is_err());
    }
    #[test]
    fn rejects_bad_backend() {
        let mut c = Config {
            device: "x".into(),
            watched_paths: vec!["/Books".into()],
            deploy: Default::default(),
            output: Default::default(),
        };
        c.deploy.backend = "ftp".into();
        assert!(c.validate().is_err());
    }
    #[test]
    fn rejects_equal_suffixes() {
        let mut c = Config {
            device: "x".into(),
            watched_paths: vec!["/Books".into()],
            deploy: Default::default(),
            output: Default::default(),
        };
        c.output.digest_suffix = c.output.annotated_suffix.clone();
        assert!(c.validate().is_err());
    }
}
