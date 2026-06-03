//! `rmapps bujo` — generate a year (or one month) of bullet-journal PDFs and
//! deploy them to the reMarkable cloud via the native client.
//!
//! Ported from `rmbujo`'s old CLI. Deploy semantics (preserving on-device ink):
//! - default: `upsert` every generated PDF, EXCEPT monthly notebooks older than
//!   the current calendar month when deploying the current year — those are kept
//!   on-device and never re-uploaded (so a deploy can't clobber a past month's
//!   ink). Pass `--from-month 1` to force every month.
//! - `--only-month N`: `upsert` month N's PDF; `create_if_missing` the
//!   non-monthly extras (future log / collection / reference); all other months
//!   untouched.
//! - `--from-month N`: filter the upload set to monthly PDFs with month >= N plus
//!   all non-monthly, then `upsert`. Overrides the default current-month floor
//!   (use `--from-month 1` to deploy the whole year, past months included).

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Datelike;
use clap::Args;

use rmbujo::{calendar, ics, notebooks};

use crate::cloud::{self, Cloud};
use crate::config::Config;

#[derive(Args, Default)]
pub struct BujoArgs {
    /// Regenerate only this single month (1..=12) and deploy just it (upsert).
    #[arg(long)]
    pub month: Option<u32>,
    /// Upload monthly notebooks for this month (1..=12) and later only; earlier
    /// months are skipped on upload (kept on-device). Non-monthly notebooks are
    /// always uploaded. Overrides the default current-month floor; pass
    /// `--from-month 1` to force the whole year (past months included).
    #[arg(long = "from-month")]
    pub from_month: Option<u32>,
    /// Sync ONLY this month's notebook (upsert); the future log / collection /
    /// reference are created only if missing. All other months are left alone.
    #[arg(long = "only-month")]
    pub only_month: Option<u32>,
    /// Override the destination folder (e.g. `/2026`). Defaults to
    /// `/{base_folder}/{year}` (or `/{year}` when base is empty).
    #[arg(long)]
    pub target: Option<String>,
    /// Re-fetch ICS feeds (otherwise the cached snapshot is reused).
    #[arg(long = "refresh-feeds")]
    pub refresh_feeds: bool,
}

impl BujoArgs {
    /// Construct args for a sync run, given the current calendar month.
    ///
    /// - `month_window = true`: sync ONLY the current month (upsert), creating the
    ///   non-monthly extras if missing. All other months untouched.
    /// - `month_window = false`: whole-year default args. `run` then applies its
    ///   current-month floor, so months older than the current month are never
    ///   re-uploaded (left on-device, ink intact) when deploying the current year.
    pub fn for_sync(month_window: bool, current_month: u32) -> Self {
        if month_window {
            Self {
                only_month: Some(current_month),
                ..Self::default()
            }
        } else {
            Self::default()
        }
    }
}

/// Default cloud folder for a year's PDFs: `/{base}/{year}`, or `/{year}` when
/// `base` is empty. Ported from rmbujo's `cloud_target`.
fn cloud_target(base: &str, year: i32) -> String {
    let base = base.trim().trim_matches('/');
    if base.is_empty() {
        format!("/{year}")
    } else {
        format!("/{base}/{year}")
    }
}

/// The month number of a generated monthly notebook PDF, or `None` for the
/// non-monthly notebooks (future log, collection, reference). Monthly files are
/// named `"YYYY.MM <Month>.pdf"`.
fn monthly_pdf_month(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let rest = name.split_once('.')?.1; // "MM <Month>.pdf"
    let mm = rest.split_once(' ')?.0; // "MM"
    mm.parse::<u32>().ok().filter(|m| (1..=12).contains(m))
}

/// The lowest month number whose notebook should be uploaded, or `None` for "no
/// floor" (upload every month).
///
/// An explicit `--from-month` (`requested`) always wins. Otherwise, when the
/// year being deployed is the *current* calendar year, the floor is the current
/// month — so a deploy never re-uploads (and clobbers the ink of) a month that's
/// already past. A future or past `deploy_year` has no floor: a future year's
/// months are all still ahead, and re-deploying a past year is an explicit act.
fn upload_floor(
    requested: Option<u32>,
    deploy_year: i32,
    current_year: i32,
    current_month: u32,
) -> Option<u32> {
    requested.or_else(|| (deploy_year == current_year).then_some(current_month))
}

pub fn run(args: BujoArgs, cfg: &Config) -> Result<()> {
    let bujo = cfg
        .bujo
        .as_ref()
        .context("no [bujo] section in rmapps config")?;
    bujo.validate()?;

    let out_dir = crate::config::cache_dir("bujo")?;
    let year = bujo.year;
    let target = match &args.target {
        Some(t) => t.clone(),
        None => cloud_target(&bujo.deploy.base_folder, year),
    };
    // backend == "none" means "generate only, skip upload".
    let upload = bujo.deploy.backend.as_str() != "none";

    // Single-month mode: build + upsert just that month.
    if let Some(month) = args.month {
        anyhow::ensure!((1..=12).contains(&month), "--month must be 1..=12 (got {month})");
        let events = ics::build_event_map(bujo, &out_dir, args.refresh_feeds, &ics::fetch::UreqFetcher)?;
        let name = calendar::MONTH_NAMES[month as usize];
        let pdf = out_dir.join(format!("{year}.{month:02} {name}.pdf"));
        notebooks::month::build_month_pdf(bujo, month, &events, &pdf)?;
        if upload {
            let cl = Cloud::from_stored()?;
            cl.upsert(&target, &cloud::doc_name(&pdf)?, std::fs::read(&pdf)?)?;
            println!("Deployed month {month} to {target}");
        } else {
            println!("Generated month {month}: {} (upload skipped)", pdf.display());
        }
        return Ok(());
    }

    // Whole-year generate.
    let mut paths = {
        let _s = tracing::info_span!("bujo.generate_year").entered();
        rmbujo::generate::generate_year(bujo, &out_dir, args.refresh_feeds)?
    };

    if let Some(only) = args.only_month {
        anyhow::ensure!((1..=12).contains(&only), "--only-month must be 1..=12 (got {only})");
    }
    if let Some(from) = args.from_month {
        anyhow::ensure!((1..=12).contains(&from), "--from-month must be 1..=12 (got {from})");
    }
    // The floor drops monthly notebooks before `floor` from the UPLOAD set (they
    // are still generated on disk). Skipped when `--only-month` is set.
    let now = chrono::Local::now();
    if let (Some(from), None) = (
        upload_floor(args.from_month, year, now.year(), now.month()),
        args.only_month,
    ) {
        paths.retain(|p| monthly_pdf_month(p).is_none_or(|m| m >= from));
    }

    if !upload {
        println!("Generated {} PDF(s) in {} (upload skipped)", paths.len(), out_dir.display());
        return Ok(());
    }

    let cl = Cloud::from_stored()?;
    if let Some(only) = args.only_month {
        // ONLY-MONTH mode: upsert this month's notebook; create-if-missing the
        // non-monthly extras; every other month untouched.
        let month_pdfs: Vec<_> = paths
            .iter()
            .filter(|p| monthly_pdf_month(p) == Some(only))
            .cloned()
            .collect();
        let extras: Vec<_> = paths
            .iter()
            .filter(|p| monthly_pdf_month(p).is_none())
            .cloned()
            .collect();
        {
            let _s = tracing::info_span!("bujo.upload", mode = "only_month").entered();
            let mut folders = cloud::FolderIds::new(&cl);
            let target_id = folders.get(&target)?;
            for pdf in &month_pdfs {
                cl.upsert_in(&target_id, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
            for pdf in &extras {
                cl.create_if_missing_in(&target_id, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
        println!(
            "Synced month {only} ({} PDF) + {} extra(s) if missing to {target}",
            month_pdfs.len(),
            extras.len()
        );
    } else {
        {
            let _s = tracing::info_span!("bujo.upload", docs = paths.len()).entered();
            let mut folders = cloud::FolderIds::new(&cl);
            let target_id = folders.get(&target)?;
            for pdf in &paths {
                cl.upsert_in(&target_id, &cloud::doc_name(pdf)?, std::fs::read(pdf)?)?;
            }
        }
        println!("Deployed {} PDF(s) to {target}", paths.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_target_appends_year() {
        assert_eq!(cloud_target("/rmbujo", 2026), "/rmbujo/2026");
        assert_eq!(cloud_target("", 2026), "/2026");
        assert_eq!(cloud_target("/", 2026), "/2026");
    }

    #[test]
    fn monthly_pdf_month_classifies_files() {
        assert_eq!(monthly_pdf_month(Path::new("2026.01 January.pdf")), Some(1));
        assert_eq!(monthly_pdf_month(Path::new("2026.12 December.pdf")), Some(12));
        assert_eq!(monthly_pdf_month(Path::new("/x/2026.05 May.pdf")), Some(5));
        assert_eq!(monthly_pdf_month(Path::new("2026 Future Log.pdf")), None);
        assert_eq!(monthly_pdf_month(Path::new("2026 Reference.pdf")), None);
    }

    #[test]
    fn for_sync_month_window_targets_only_current_month() {
        let args = BujoArgs::for_sync(true, 6);
        assert_eq!(args.only_month, Some(6));
        assert_eq!(args.from_month, None);
    }

    #[test]
    fn for_sync_without_window_uses_whole_year_args() {
        // Non-window sync produces plain whole-year args; `run`'s current-month
        // floor (see upload_floor) is what skips past months.
        let args = BujoArgs::for_sync(false, 6);
        assert_eq!(args.from_month, None);
        assert_eq!(args.only_month, None);
    }

    #[test]
    fn upload_floor_defaults_to_current_month_for_current_year() {
        // Deploying 2026 in June 2026: floor at month 6, so Jan..May are skipped.
        assert_eq!(upload_floor(None, 2026, 2026, 6), Some(6));
    }

    #[test]
    fn upload_floor_explicit_request_overrides_default() {
        // `--from-month 1` forces the whole year even in the current year.
        assert_eq!(upload_floor(Some(1), 2026, 2026, 6), Some(1));
        // An explicit floor above the current month is honored too.
        assert_eq!(upload_floor(Some(9), 2026, 2026, 6), Some(9));
    }

    #[test]
    fn upload_floor_no_floor_for_other_years() {
        // A future year: every month is still ahead — upload all of them.
        assert_eq!(upload_floor(None, 2027, 2026, 6), None);
        // A past year deploy is explicit; don't silently drop its months either.
        assert_eq!(upload_floor(None, 2025, 2026, 6), None);
        // ...but an explicit --from-month still applies regardless of year.
        assert_eq!(upload_floor(Some(3), 2027, 2026, 6), Some(3));
    }

    #[test]
    fn from_month_filter_keeps_current_future_and_non_monthly() {
        let all = [
            "2026 Future Log.pdf",
            "2026.01 January.pdf",
            "2026.05 May.pdf",
            "2026.12 December.pdf",
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
                "2026 Reference.pdf",
            ]
        );
    }
}
