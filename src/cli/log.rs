use std::{
    io::{self, Read},
    path::Path,
};

use anyhow::{Context, bail};
use notema_domain::{MOOD_RANGE, Metadata, validate_feelings};
use notema_storage::JournalStore;

use crate::{AppResult, startup, tui};

use super::{Cli, LogArgs, plural};

pub(super) fn run(cli: &Cli, args: &LogArgs, stdin_is_pipe: bool) -> AppResult<()> {
    let body_from_args = !args.body.is_empty();
    if body_from_args && stdin_is_pipe {
        bail!("entry text cannot be combined with piped stdin");
    }

    let startup::Startup {
        config_path,
        config,
        store,
        ..
    } = startup::load_existing(cli.config.as_deref())?;
    let journal = args
        .journal
        .as_deref()
        .or(config.journal.default.as_deref())
        .context("no journal specified; pass --journal or set one with `notema use <name>`")?;
    validate_existing_journal(&config.journal.path, journal)?;
    let tags = comma_separated_values(&args.tag);
    let people = comma_separated_values(&args.person);
    let activities = comma_separated_values(&args.activity);
    let feelings = validate_feelings(
        args.feeling
            .iter()
            .flat_map(|f| f.split(','))
            .map(str::trim)
            .filter(|f| !f.is_empty()),
    )
    .map_err(anyhow::Error::msg)?;
    let mood = if let Some(score) = args.mood {
        if !MOOD_RANGE.contains(&score) {
            bail!(
                "--mood must be between {} and {}, got {score}",
                MOOD_RANGE.start(),
                MOOD_RANGE.end()
            );
        }
        Some(score)
    } else {
        None
    };
    let metadata = Metadata {
        tags,
        people,
        activities,
        feelings,
        mood,
        starred: false,
        location: None,
    };

    // No inline text: compose interactively in the fullscreen built-in editor. It
    // handles asset ingest and status on save, so nothing is printed here.
    if !body_from_args && !stdin_is_pipe {
        let journal = journal.to_string();
        return tui::run_compose(config_path, config, store, journal, metadata);
    }

    let body = if body_from_args {
        args.body.join(" ")
    } else {
        let mut body = String::new();
        io::stdin().read_to_string(&mut body)?;
        body
    };
    let created = store.create_entry(
        notema_storage::EntryDraft::new(journal, &body, &metadata),
        notema_storage::EntryAssetOptions {
            download_remote: config.attachments.download_remote_images,
            replace_offline: false,
        },
    )?;
    if !created.assets.is_noop() {
        eprintln!("{}", asset_report_message(&created.assets));
    }
    println!("{}", created.path.display());
    Ok(())
}

fn asset_report_message(report: &notema_storage::AssetReport) -> String {
    let mut parts = Vec::new();
    let images_stored = report.images_stored();
    if images_stored > 0 {
        parts.push(format!(
            "{} {} stored",
            images_stored,
            plural(images_stored, "image", "images")
        ));
    }
    if report.attachments_stored > 0 {
        parts.push(format!(
            "{} {} stored",
            report.attachments_stored,
            plural(report.attachments_stored, "attachment", "attachments")
        ));
    }
    if report.removed > 0 {
        parts.push(format!("{} removed", report.removed));
    }
    let images_not_stored = report.images_not_stored();
    if images_not_stored > 0 {
        parts.push(format!(
            "{} {} not stored",
            images_not_stored,
            plural(images_not_stored, "image", "images")
        ));
    }
    let attachments_not_stored = report.attachments_not_stored();
    if attachments_not_stored > 0 {
        parts.push(format!(
            "{} {} not stored",
            attachments_not_stored,
            plural(attachments_not_stored, "attachment", "attachments")
        ));
    }
    parts.join("; ")
}

fn comma_separated_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn validate_existing_journal(root: &Path, journal: &str) -> AppResult<()> {
    let journal = JournalStore::validate_journal_name(journal)?;
    let path = root.join(&journal);
    if !path.is_dir() {
        bail!(
            "journal '{journal}' does not exist; create it or pick another with `notema use <name>`"
        );
    }
    Ok(())
}
