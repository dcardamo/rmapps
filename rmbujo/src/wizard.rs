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
    // The caller validates and creates the directory after this returns, so
    // invalid input doesn't leave an orphan folder behind.
    Ok((config, out_dir, config_path))
}
