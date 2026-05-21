//! Reader theme: TOML palette -> map + CSS custom properties.
use std::collections::BTreeMap;

const READER_TOML: &str = include_str!("../themes/reader.toml");
pub type Palette = BTreeMap<String, String>;

pub fn load_theme(name_or_path: &str) -> anyhow::Result<Palette> {
    let content = match name_or_path {
        "reader" => READER_TOML.to_string(),
        p if p.ends_with(".toml") => std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("theme not found: {name_or_path} ({e})"))?,
        other => anyhow::bail!("unknown theme {other:?}; use 'reader' or a path to a .toml"),
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
