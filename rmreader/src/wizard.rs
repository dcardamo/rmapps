//! Interactive `init` wizard. `assemble` is pure (testable); `run_wizard` prompts.
use std::path::PathBuf;

use crate::config::{
    Config, ContentConfig, DeployConfig, FeedConfig, ImagesConfig, LibraryConfig, ReadwiseConfig,
};

pub struct Answers {
    pub output_dir: String,
    pub device: String,
    pub token: String,
    pub library_locations: Vec<String>,
    pub library_max: u32,
    pub feed_enabled: bool,
    pub feed_max: u32,
    pub images_enabled: bool,
    pub deploy_backend: String,
    pub library_folder: String,
    pub feed_folder: String,
}

pub fn assemble(a: Answers) -> (Config, PathBuf, PathBuf) {
    let config = Config {
        device: a.device,
        output_dir: a.output_dir.clone(),
        theme: "reader".into(),
        readwise: ReadwiseConfig { token: a.token },
        library: LibraryConfig {
            locations: a.library_locations,
            max_items: a.library_max,
        },
        feed: FeedConfig {
            enabled: a.feed_enabled,
            max_items: a.feed_max,
        },
        images: ImagesConfig {
            enabled: a.images_enabled,
            ..ImagesConfig::default()
        },
        content: ContentConfig::default(),
        deploy: DeployConfig {
            backend: a.deploy_backend,
            library_folder: a.library_folder,
            feed_folder: a.feed_folder,
        },
    };
    let out_dir = PathBuf::from(a.output_dir);
    let config_path = out_dir.join("rmreader.toml");
    (config, out_dir, config_path)
}

/// Prompt, validate the token, and return Config + paths. Caller writes files.
pub fn run_wizard(
    transport: &dyn crate::readwise::HttpTransport,
) -> anyhow::Result<(Config, PathBuf, PathBuf)> {
    use dialoguer::{Confirm, Input};

    let output_dir: String = Input::new()
        .with_prompt("Output directory")
        .default(".".into())
        .interact_text()?;
    let device: String = Input::new()
        .with_prompt("Device (paper-pro-move|paper-pro)")
        .default("paper-pro-move".into())
        .interact_text()?;

    println!("Get your Readwise token at https://readwise.io/access_token");
    let token: String = Input::new().with_prompt("Readwise token").interact_text()?;
    crate::readwise::validate_token(transport, &token)?; // fail fast on bad token

    let library_max: u32 = Input::new()
        .with_prompt("Library: max items")
        .default(100)
        .interact_text()?;
    let feed_enabled: bool = Confirm::new()
        .with_prompt("Generate a Feed PDF?")
        .default(true)
        .interact()?;
    let feed_max: u32 = Input::new()
        .with_prompt("Feed: max items")
        .default(100)
        .interact_text()?;
    let images_enabled: bool = Confirm::new()
        .with_prompt("Include images?")
        .default(true)
        .interact()?;
    let deploy_backend: String = Input::new()
        .with_prompt("Deploy backend (none|rmapi)")
        .default("none".into())
        .interact_text()?;
    let (library_folder, feed_folder) = if deploy_backend == "rmapi" {
        let lf: String = Input::new()
            .with_prompt("reMarkable folder for Library")
            .default("/Reader".into())
            .interact_text()?;
        let ff: String = Input::new()
            .with_prompt("reMarkable folder for Feed")
            .default(lf.clone())
            .interact_text()?;
        (lf, ff)
    } else {
        (String::new(), String::new())
    };

    Ok(assemble(Answers {
        output_dir,
        device,
        token,
        library_locations: vec!["new".into(), "later".into(), "shortlist".into()],
        library_max,
        feed_enabled,
        feed_max,
        images_enabled,
        deploy_backend,
        library_folder,
        feed_folder,
    }))
}
