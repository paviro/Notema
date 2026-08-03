use jiff::Zoned;
use jiff::civil::Date;
use notema_domain::entry_date_from_path;

use notema_domain::Entry;

/// Parse an RFC3339 timestamp, preserving its original offset.
pub fn parse_entry_timestamp(value: &str) -> Option<Zoned> {
    notema_domain::Timestamp::parse(value).parsed
}

/// Render a [`Zoned`] as a plain RFC3339 string carrying its offset (e.g.
/// `2026-07-01T10:23:00+02:00`), with no RFC 9557 zone/offset annotation — the
/// exact shape front matter stores and `parse_entry_timestamp` reads back.
pub(crate) fn to_offset_rfc3339(datetime: &Zoned) -> String {
    datetime
        .timestamp()
        .display_with_offset(datetime.offset())
        .to_string()
}

fn format_date_human(date: Date) -> String {
    date.strftime("%A, %-d %B %Y").to_string()
}

pub fn entry_timestamp_label(entry: &Entry) -> String {
    entry
        .created_time()
        .map(|timestamp| {
            format!(
                "{}, {}",
                format_date_human(timestamp.date()),
                timestamp.strftime("%H:%M")
            )
        })
        .or_else(|| entry_date_from_path(&entry.path).map(format_date_human))
        .unwrap_or_else(|| "Entry".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(created_at: Option<&str>, path: &str) -> Entry {
        Entry {
            id: "id".to_string(),
            journal: "work".to_string(),
            path: PathBuf::from(path),
            encryption_state: notema_domain::EntryEncryptionState::Plain,
            created_at: created_at.map(notema_domain::Timestamp::parse),
            edited_at: None,
            preview: String::new(),
            activities: Vec::new(),
            feelings: Vec::new(),
            people: Vec::new(),
            tags: Vec::new(),
            mood: None,
            starred: false,
            location: None,
            weather: None,
            celestial: None,
            air_quality: None,
            import: None,
            body: String::new(),
            word_count: 0,
            search_haystack: String::new(),
            warning: None,
        }
    }

    #[test]
    fn timestamp_label_prefers_created_timestamp() {
        let entry = entry(Some("2026-07-01T10:23:00+02:00"), "work/2026-01-01/id.md");

        assert_eq!(
            entry_timestamp_label(&entry),
            "Wednesday, 1 July 2026, 10:23"
        );
    }
}
