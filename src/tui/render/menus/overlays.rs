use super::*;

const METADATA_MENU_ITEMS: [(&str, &str); 6] = [
    ("t", "Tags"),
    ("p", "People"),
    ("a", "Activities"),
    ("f", "Feelings"),
    ("m", "Mood"),
    ("l", "Location"),
];

fn metadata_menu_rows() -> Vec<Vec<String>> {
    METADATA_MENU_ITEMS
        .iter()
        .map(|(key, label)| vec![key.to_string(), label.to_string()])
        .collect()
}

fn metadata_menu_dialog<'a>(
    theme: &'a Theme,
    rows: &'a [Vec<String>],
    mode: MetadataMenuMode,
) -> TableDialog<'a> {
    TableDialog {
        theme,
        title: "Add Metadata",
        headers: &["Key", "Add"],
        rows,
        key_col: 0,
        footer: mode.footer(),
    }
}

/// Where the metadata chooser is shown. The editor gates its metadata keys behind
/// this popup ("press key"); the viewer's keys work at any time, so there the popup
/// is only a reference ("reference").
#[derive(Clone, Copy)]
pub(crate) enum MetadataMenuMode {
    Editor,
    Viewer,
}

impl MetadataMenuMode {
    fn footer(self) -> &'static str {
        match self {
            Self::Editor => "press key · esc",
            Self::Viewer => "reference · esc",
        }
    }
}

/// Draw the "Add metadata" chooser: a centered popup whose highlighted letters open
/// the tags/people/activities/feelings/mood dialogs, laid out as a table matching
/// the insights tabs. Shared by the internal editor and the entry viewer.
pub(crate) fn draw_metadata_menu(
    theme: &Theme,
    frame: &mut Frame<'_>,
    mode: MetadataMenuMode,
    hovered_row: Option<usize>,
) {
    let rows = metadata_menu_rows();
    // The chooser always fits, so it never scrolls.
    let mut scroll = 0;
    draw_table_dialog(
        frame,
        &metadata_menu_dialog(theme, &rows, mode),
        &mut scroll,
        hovered_row,
    );
}

pub(crate) fn metadata_menu_interactions(
    theme: &Theme,
    frame_area: Rect,
    mode: MetadataMenuMode,
) -> MenuInteractions {
    let rows = metadata_menu_rows();
    table_dialog_interactions(frame_area, &metadata_menu_dialog(theme, &rows, mode), 0)
}

const SETTINGS_MENU_ITEMS: [(&str, &str); 1] = [("t", "Theme…")];

fn settings_menu_rows() -> Vec<Vec<String>> {
    SETTINGS_MENU_ITEMS
        .iter()
        .map(|(key, label)| vec![key.to_string(), label.to_string()])
        .collect()
}

fn settings_menu_dialog<'a>(theme: &'a Theme, rows: &'a [Vec<String>]) -> TableDialog<'a> {
    TableDialog {
        theme,
        title: "Settings",
        headers: &["Key", "Setting"],
        rows,
        key_col: 0,
        footer: "enter select · esc close",
    }
}

/// Draw the settings menu: a centered chooser whose rows open the settings
/// dialogs. Same table popup as the metadata menu.
pub(crate) fn draw_settings_menu(theme: &Theme, frame: &mut Frame<'_>, hovered_row: Option<usize>) {
    let rows = settings_menu_rows();
    // The menu always fits, so it never scrolls.
    let mut scroll = 0;
    draw_table_dialog(
        frame,
        &settings_menu_dialog(theme, &rows),
        &mut scroll,
        hovered_row,
    );
}

pub(crate) fn settings_menu_interactions(theme: &Theme, frame_area: Rect) -> MenuInteractions {
    let rows = settings_menu_rows();
    table_dialog_interactions(frame_area, &settings_menu_dialog(theme, &rows), 0)
}
