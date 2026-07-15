use std::collections::HashSet;

use anyhow::Context;
use notema_storage::JournalStore;

use crate::{AppResult, startup};

use super::{Cli, DayoneArgs, encryption, plural, unlock_if_encrypted};

pub(super) fn run_dayone(cli: &Cli, args: &DayoneArgs) -> AppResult<()> {
    let startup::Startup {
        config, mut store, ..
    } = startup::load_existing(cli.config.as_deref())?;
    let journal = args
        .journal
        .as_deref()
        .or(config.journal.default.as_deref())
        .context("no journal specified; pass --journal or set one with `notema use <name>`")?;
    // Validate the name only — the importer creates the journal if it's missing.
    let journal = JournalStore::validate_journal_name(journal)?;

    // Duplicate detection reads existing entries' `[import]` provenance, which
    // on an encrypted store requires the unlocked identity.
    unlock_if_encrypted(&mut store)?;

    let batch = notema_import::parse_dayone(&args.path)?;
    if !store
        .list_journals()?
        .iter()
        .any(|existing| existing.name == journal)
    {
        store.create_journal(&journal)?;
    }
    let mut seen: HashSet<_> = store.scan_import_sources()?.into_iter().collect();
    let mut report = ImportReport::default();
    for warning in batch.warnings {
        report
            .failures
            .push(format!("{}: {}", warning.entry_id, warning.message));
    }
    let total = batch.entries.len();
    let mut progress = encryption::cli_progress("entries");
    for (idx, entry) in batch.entries.into_iter().enumerate() {
        progress(idx, total);
        if !seen.insert(entry.provenance.clone()) {
            report.skipped_duplicate += 1;
            continue;
        }
        let created = store.create_entry(
            notema_storage::EntryDraft {
                journal: &journal,
                body: &entry.body,
                metadata: &entry.metadata,
                created_at: Some(entry.created_at),
                edited_at: Some(entry.edited_at),
                timezone: entry.timezone.as_deref(),
                location: entry.location.as_ref(),
                weather: entry.weather.as_ref(),
                celestial: entry.celestial.as_ref(),
                air_quality: None,
                writing_seconds: entry.writing_seconds,
                import: Some(&entry.provenance),
            },
            notema_storage::EntryAssetOptions {
                download_remote: args.download_images,
                replace_offline: args.download_images,
            },
        )?;
        report.imported += 1;
        report.attachments_copied += created.assets.attachments_stored;
        report.images_stored += created.assets.images_stored();
        for failure in created.assets.failed {
            match failure {
                notema_storage::AssetFailure::RemoteUnavailable { .. } => {
                    report.remote_images_skipped += 1;
                }
                notema_storage::AssetFailure::Ingest { source, error } => {
                    report.images_failed += 1;
                    report
                        .failures
                        .push(format!("{}: {source}: {error}", entry.provenance.id));
                }
                notema_storage::AssetFailure::AttachmentIngest { source, error } => {
                    report.attachments_failed += 1;
                    report
                        .failures
                        .push(format!("{}: {source}: {error}", entry.provenance.id));
                }
            }
        }
    }
    progress(total, total);

    println!(
        "{}",
        import_report_summary(&report, &journal, args.download_images)
    );
    for failure in &report.failures {
        eprintln!("  ! {failure}");
    }
    Ok(())
}

fn import_report_summary(report: &ImportReport, journal: &str, download_images: bool) -> String {
    let mut parts = vec![format!(
        "Imported {} {} into '{journal}'",
        report.imported,
        plural(report.imported, "entry", "entries"),
    )];
    if report.skipped_duplicate > 0 {
        parts.push(format!(
            "{} already imported (skipped)",
            report.skipped_duplicate
        ));
    }
    if report.images_stored > 0 {
        parts.push(format!(
            "{} {} stored",
            report.images_stored,
            plural(report.images_stored, "image", "images")
        ));
    }
    if report.attachments_copied > 0 {
        parts.push(format!(
            "{} audio/video/pdf {} copied",
            report.attachments_copied,
            plural(report.attachments_copied, "attachment", "attachments")
        ));
    }
    if report.attachments_failed > 0 {
        parts.push(format!(
            "{} audio/video/pdf {} not copied",
            report.attachments_failed,
            plural(report.attachments_failed, "attachment", "attachments")
        ));
    }
    if report.remote_images_skipped > 0 {
        if download_images {
            parts.push(format!(
                "{} offline {} replaced with [Offline Image]",
                report.remote_images_skipped,
                plural(report.remote_images_skipped, "image", "images")
            ));
        } else {
            parts.push(format!(
                "{} remote {} left as links (pass --download-images to fetch)",
                report.remote_images_skipped,
                plural(report.remote_images_skipped, "link", "links")
            ));
        }
    }
    if report.images_failed > 0 {
        parts.push(format!(
            "{} {} not stored",
            report.images_failed,
            plural(report.images_failed, "image", "images")
        ));
    }
    parts.join("; ")
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ImportReport {
    imported: usize,
    skipped_duplicate: usize,
    images_stored: usize,
    images_failed: usize,
    remote_images_skipped: usize,
    attachments_copied: usize,
    attachments_failed: usize,
    failures: Vec<String>,
}
