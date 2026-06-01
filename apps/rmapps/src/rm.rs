//! `rmapps rm PATH [--recursive]` — delete the document OR folder at PATH.
//!
//! Resolves PATH's parent folder, finds the entry by leaf name, and removes it
//! via the native client. `--recursive` first removes a folder's children
//! (recursing into subfolders) so no documents are orphaned. A PATH that does
//! not exist is reported, not treated as an error.

use anyhow::{anyhow, Result};
use clap::Args;

use crate::cloud::Cloud;

#[derive(Args)]
pub struct RmArgs {
    /// Document or folder path to delete, e.g. `/Books/Old` or `/--help`.
    path: String,
    /// If PATH is a folder, delete its contents first (otherwise a non-empty
    /// folder's documents would be orphaned).
    #[arg(long)]
    recursive: bool,
}

pub fn run(args: RmArgs) -> Result<()> {
    let cloud = Cloud::from_stored()?;

    // Split PATH into parent folder + leaf name.
    let trimmed = args.path.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Err(anyhow!("refusing to delete the cloud root"));
    }
    let (parent_path, leaf) = match trimmed.rsplit_once('/') {
        Some((p, l)) => (p.to_string(), l.to_string()),
        None => (String::new(), trimmed.to_string()),
    };

    // Resolve the parent read-only; missing parent ⇒ the target can't exist.
    let parent_id = match cloud.resolve_folder(&parent_path)? {
        Some(id) => id,
        None => {
            println!("{}: not found", args.path);
            return Ok(());
        }
    };

    // Find the target entry by leaf name among the parent's children.
    let entries = cloud.block_on(cloud.client().ls(&parent_id))?;
    let Some(target) = entries.into_iter().find(|e| e.name == leaf) else {
        println!("{}: not found", args.path);
        return Ok(());
    };

    if target.is_folder {
        if args.recursive {
            remove_folder_recursive(&cloud, &target.id, &args.path)?;
        }
        // Remove the (now-empty, if --recursive) folder itself.
        cloud.block_on(cloud.client().rm(&target.id))?;
        println!("deleted folder {}", args.path);
    } else {
        cloud.block_on(cloud.client().rm(&target.id))?;
        println!("deleted file {}", args.path);
    }
    Ok(())
}

/// Recursively remove every child of `folder_id`, deepest first, so that
/// documents are never orphaned. `display_path` is used only for output.
fn remove_folder_recursive(cloud: &Cloud, folder_id: &str, display_path: &str) -> Result<()> {
    let children = cloud.block_on(cloud.client().ls(folder_id))?;
    for child in children {
        let child_path = format!("{}/{}", display_path.trim_end_matches('/'), child.name);
        if child.is_folder {
            remove_folder_recursive(cloud, &child.id, &child_path)?;
            cloud.block_on(cloud.client().rm(&child.id))?;
            println!("deleted folder {child_path}");
        } else {
            cloud.block_on(cloud.client().rm(&child.id))?;
            println!("deleted file {child_path}");
        }
    }
    Ok(())
}
