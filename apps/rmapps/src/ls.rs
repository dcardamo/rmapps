//! `rmapps ls [PATH]` — list the entries directly under a cloud folder.
//!
//! Prints one line per entry: `d  <name>` for folders, `f  <name>` for files.
//! PATH defaults to the cloud root (`/`). Resolution is read-only (no folder is
//! created); an unresolvable PATH prints `(no such folder)`.

use anyhow::Result;
use clap::Args;

use crate::cloud::Cloud;

#[derive(Args)]
pub struct LsArgs {
    /// Folder to list (default: the cloud root `/`).
    #[arg(default_value = "/")]
    path: String,
}

pub fn run(args: LsArgs) -> Result<()> {
    let cloud = Cloud::from_stored()?;

    // Resolve PATH → folder id without creating anything. Root ("" / "/") → "".
    let folder_id = match cloud.resolve_folder(&args.path)? {
        Some(id) => id,
        None => {
            println!("(no such folder)");
            return Ok(());
        }
    };

    let entries = cloud.block_on(cloud.client().ls(&folder_id))?;
    for e in entries {
        let kind = if e.is_folder { 'd' } else { 'f' };
        println!("{kind}  {}", e.name);
    }
    Ok(())
}
