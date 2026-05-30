//! rmbujo CLI: `new` wizard, or regenerate from a config path.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::{calendar, config, deploy, generate, ics, notebooks, wizard};

#[derive(Parser)]
#[command(
    name = "rmbujo",
    version,
    about = "Dot-grid bullet journal PDF generator for reMarkable",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// Path to an existing rmbujo.toml to regenerate.
    config: Option<PathBuf>,
    /// Re-fetch ICS feeds (otherwise cached feeds are reused on regenerate).
    #[arg(long = "refresh-feeds")]
    refresh_feeds: bool,
    /// Override the rmapi destination folder for the whole-year regenerate
    /// (e.g. `/2026`). Defaults to the year subfolder under deploy.base_folder.
    /// Mirrors `month --target`; the saturn job uses it to publish each year
    /// into its own root folder.
    #[arg(long)]
    target: Option<String>,
    /// Only upload monthly notebooks for this month (1..=12) and later; earlier
    /// months are skipped on upload so they stop syncing once they're in the
    /// past (they stay on-device, nothing is deleted). The future log,
    /// collection, and reference are always uploaded. The saturn job passes the
    /// current month so only the current + future months sync.
    #[arg(long)]
    from_month: Option<u32>,
    #[command(subcommand)]
    command: Option<Command>,
}

/// The month number of a generated monthly notebook PDF, or `None` for the
/// non-monthly notebooks (future log, collection, reference). Monthly files are
/// named `"YYYY.MM <Month>.pdf"` (see `generate::generate_year`), so the digits
/// between the first `.` and the following space are the month.
fn monthly_pdf_month(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let rest = name.split_once('.')?.1; // "MM <Month>.pdf"
    let mm = rest.split_once(' ')?.0; // "MM"
    mm.parse::<u32>().ok().filter(|m| (1..=12).contains(m))
}

#[derive(Subcommand)]
enum Command {
    /// Create a new year interactively.
    New,
    /// Regenerate a single month and upload it. Used by the saturn hourly job
    /// (current month + next month → cloud root) so on-device handwriting on
    /// the current monthly notebook stays current with calendar edits without
    /// rebuilding the whole year.
    Month {
        /// Path to rmbujo.toml.
        config: PathBuf,
        /// Month number, 1..=12. The year comes from the config.
        #[arg(long)]
        month: u32,
        /// Override the rmapi destination folder (e.g. `/` for cloud root).
        /// Defaults to the year subfolder under `deploy.base_folder`.
        #[arg(long)]
        target: Option<String>,
        /// Re-fetch ICS feeds (otherwise the cached snapshot is reused).
        #[arg(long = "refresh-feeds")]
        refresh_feeds: bool,
    },
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    // try_parse_from returns Err for --help/--version (DisplayHelp/DisplayVersion);
    // e.exit() handles those correctly (print + exit 0) instead of propagating as anyhow::Error.
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| e.exit());
    match (cli.command, cli.config, cli.refresh_feeds) {
        (
            Some(Command::Month {
                config: cfg_path,
                month,
                target,
                refresh_feeds,
            }),
            _,
            _,
        ) => run_month(&cfg_path, month, target.as_deref(), refresh_feeds),
        (Some(Command::New), _, _) => {
            let (config, out_dir, config_path) = wizard::run_wizard()?;
            config.validate()?;
            std::fs::create_dir_all(&out_dir)?;
            config::dump(&config, &config_path)?;
            // First run: always fetch feeds fresh.
            let paths = generate::generate_year(&config, &out_dir, true)?;
            deploy::get_deployer(&config)?.deploy(&paths)?;
            println!("Wrote {} PDFs to {}", paths.len(), out_dir.display());
            Ok(())
        }
        (None, Some(path), refresh_feeds) => {
            let config = config::load(&path)?;
            config.validate()?;
            let out_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            // Reuse cached feeds unless --refresh-feeds was passed.
            let mut paths = generate::generate_year(&config, &out_dir, refresh_feeds)?;
            // `--from-month N` drops monthly notebooks before month N from the
            // UPLOAD set only (they are still generated on disk). Past months
            // thus stop syncing once they're behind the current month, while the
            // existing on-device copies are left untouched. Non-monthly notebooks
            // (future log, collection, reference) are always kept.
            if let Some(from) = cli.from_month {
                anyhow::ensure!(
                    (1..=12).contains(&from),
                    "--from-month must be 1..=12 (got {from})"
                );
                paths.retain(|p| monthly_pdf_month(p).is_none_or(|m| m >= from));
            }
            // Upsert the remaining PDFs: create on first run, content-only refresh
            // afterwards so on-device handwriting is preserved, and mkdir the
            // folder lazily. `--target` overrides the cloud folder (e.g. `/2026`);
            // otherwise the year subfolder under deploy.base_folder.
            match config.deploy.backend.as_str() {
                "none" => {}
                "rmapi" => {
                    let target_folder = match cli.target.as_deref() {
                        Some(t) => t.to_string(),
                        None => {
                            deploy::rmapi::cloud_target(&config.deploy.base_folder, config.year)
                        }
                    };
                    let runner = deploy::rmapi::ProcessRmapi::new()?;
                    deploy::rmapi::RmapiDeployer::new(target_folder, runner).upsert(&paths)?;
                }
                other => anyhow::bail!("unsupported deploy backend: {other:?}"),
            }
            println!("Regenerated {} PDFs in {}", paths.len(), out_dir.display());
            Ok(())
        }
        (None, None, _) => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

fn run_month(
    cfg_path: &Path,
    month: u32,
    target: Option<&str>,
    refresh_feeds: bool,
) -> anyhow::Result<()> {
    if !(1..=12).contains(&month) {
        anyhow::bail!("--month must be 1..=12 (got {month})");
    }
    let config = config::load(cfg_path)?;
    config.validate()?;
    let out_dir = cfg_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    std::fs::create_dir_all(&out_dir)?;
    let events = ics::build_event_map(&config, &out_dir, refresh_feeds, &ics::fetch::UreqFetcher)?;
    let y = config.year;
    let name = calendar::MONTH_NAMES[month as usize];
    let pdf = out_dir.join(format!("{y}.{month:02} {name}.pdf"));
    notebooks::month::build_month_pdf(&config, month, &events, &pdf)?;

    match config.deploy.backend.as_str() {
        "none" => {}
        "rmapi" => {
            let target_folder = match target {
                Some(t) => t.to_string(),
                None => deploy::rmapi::cloud_target(&config.deploy.base_folder, y),
            };
            let runner = deploy::rmapi::ProcessRmapi::new()?;
            deploy::rmapi::RmapiDeployer::new(target_folder, runner)
                .upsert(std::slice::from_ref(&pdf))?;
        }
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
    println!("Regenerated 1 PDF: {}", pdf.display());
    Ok(())
}

pub fn main() -> anyhow::Result<()> {
    run(std::env::args().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn bare_config_parses_target_override() {
        // The whole-year regenerate path accepts `--target` to force the cloud
        // folder (saturn job publishes each year into /<year>).
        let cli =
            Cli::try_parse_from(["rmbujo", "/tmp/x/rmbujo.toml", "--target", "/2026"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.config.as_deref(), Some(Path::new("/tmp/x/rmbujo.toml")));
        assert_eq!(cli.target.as_deref(), Some("/2026"));
    }

    #[test]
    fn from_month_parses() {
        let cli =
            Cli::try_parse_from(["rmbujo", "/tmp/x/rmbujo.toml", "--from-month", "5"]).unwrap();
        assert_eq!(cli.from_month, Some(5));
    }

    #[test]
    fn monthly_pdf_month_classifies_files() {
        // Monthly notebooks yield their month; everything else yields None.
        assert_eq!(monthly_pdf_month(Path::new("2026.01 January.pdf")), Some(1));
        assert_eq!(
            monthly_pdf_month(Path::new("2026.12 December.pdf")),
            Some(12)
        );
        assert_eq!(monthly_pdf_month(Path::new("/x/2026.05 May.pdf")), Some(5));
        assert_eq!(monthly_pdf_month(Path::new("2026 Future Log.pdf")), None);
        assert_eq!(
            monthly_pdf_month(Path::new("2026 Collection Template.pdf")),
            None
        );
        assert_eq!(monthly_pdf_month(Path::new("2026 Reference.pdf")), None);
    }

    #[test]
    fn from_month_filter_keeps_current_future_and_non_monthly() {
        // Simulate the upload-set filter for from_month = 5 (May).
        let all = [
            "2026 Future Log.pdf",
            "2026.01 January.pdf",
            "2026.04 April.pdf",
            "2026.05 May.pdf",
            "2026.12 December.pdf",
            "2026 Collection Template.pdf",
            "2026 Reference.pdf",
        ];
        let kept: Vec<&str> = all
            .iter()
            .copied()
            .filter(|n| monthly_pdf_month(Path::new(n)).is_none_or(|m| m >= 5))
            .collect();
        assert_eq!(
            kept,
            vec![
                "2026 Future Log.pdf",
                "2026.05 May.pdf",
                "2026.12 December.pdf",
                "2026 Collection Template.pdf",
                "2026 Reference.pdf",
            ]
        );
    }
}
