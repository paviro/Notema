//! Hint chips inserted into the reader body, and the labels they carry.

use super::*;

/// A chip is inserted into the text, never painted over it — so the link name
/// survives whole and the paragraph re-wraps to make room.
#[test]
fn hint_chips_are_inserted_without_covering_any_text() {
    let body = "Visit [labelled link](https://example.com) today.";

    let plain = render_text(app_reading(body), 80, 24);
    let mut app = app_reading(body);
    app.reader_hints.begin();
    let hinted = render_text(app, 80, 24);

    assert!(plain.contains("Visit labelled link today."));
    assert!(
        hinted.contains("Visit labelled link │ press a │ today."),
        "the chip trails the name, and every character of the sentence survives"
    );
}

/// The chip reads as a raised key: rail and label share the lift, and the label
/// wears the link colour bold but *not* underlined — it is the key to press, not
/// another link to click.
#[test]
fn a_hint_chip_reads_as_a_raised_key() {
    let mut app = app_reading("A [link](https://example.com) here.");
    app.reader_hints.begin();
    // A coloured theme, so the fg assertions bite: the terminal default leaves
    // both `md_link` and `muted` uncoloured and every cell would agree.
    app.appearance.theme = theme::test_eclipse_theme();
    let theme = app.appearance.theme.clone();
    let mut view = crate::tui::ui::ViewState::default();
    let backend = render_backend(80, 24, |frame| draw_app(frame, &mut app, &mut view));

    let (row, column) = find_text(&backend, "│ press ").expect("the chip is on screen");
    let cell = |column| backend.buffer().cell((column, row)).expect("in bounds");
    let rail = cell(column);
    // `│ press ` is eight columns, so the label sits just past it.
    let key = cell(column + 8);

    assert_eq!(key.symbol(), "a");
    assert_eq!(key.bg, rail.bg, "one lift under the whole chip");
    assert_eq!(
        Some(key.fg),
        theme.md_link().fg,
        "the label wears the link colour"
    );
    assert!(key.modifier.contains(Modifier::BOLD));
    assert!(
        !key.modifier.contains(Modifier::UNDERLINED),
        "a key to press, not a link to click"
    );
    assert_eq!(Some(rail.fg), theme.muted().fg, "the rail stays secondary");
}

/// Every openable target gets a label, across all four kinds.
#[test]
fn every_openable_target_kind_is_labelled() {
    let mut app = app_reading(
        "See [one](https://example.com) and [two](https://example.org).\n\n\
         Jump to [details](#details).\n\n## Details\n\ntail",
    );
    app.reader_hints.begin();
    let mut view = crate::tui::ui::ViewState::default();
    render_backend(80, 24, |frame| draw_app(frame, &mut app, &mut view));

    let labels: Vec<_> = view
        .reader
        .hints
        .iter()
        .map(|hint| hint.label.clone())
        .collect();
    assert_eq!(labels, vec!["a", "s", "d"]);
    // The anchor resolves to the heading it jumps to; the URLs do not.
    assert!(view.reader.hints[2].heading_line.is_some());
    assert!(view.reader.hints[0].heading_line.is_none());
}

/// The `(row, column)` where `needle` starts, scanning row-major. Returns the
/// column so a caller can step to a neighbouring cell and read its style.
fn find_text(backend: &TestBackend, needle: &str) -> Option<(u16, u16)> {
    let buffer = backend.buffer();
    let area = buffer.area();
    (area.y..area.bottom()).find_map(|row| {
        let line: String = (area.x..area.right())
            .map(|column| buffer.cell((column, row)).map_or(" ", |cell| cell.symbol()))
            .collect();
        line.find(needle)
            .map(|index| (row, area.x + line[..index].chars().count() as u16))
    })
}

/// The image label advertises the click only; the keyboard reaches it through
/// link-hint mode like every other target.
#[test]
fn image_labels_no_longer_advertise_a_digit() {
    let rendered = render_text(app_reading("Body text."), 120, 20);
    assert!(!rendered.contains("or press"));
}

/// The `open link` chip follows the entry, like the `images` chip does: offered
/// when there is something to open, absent when there is not.
#[test]
fn the_open_link_chip_follows_what_the_entry_holds() {
    // The count is recorded from the drawn body, so the first frame establishes
    // it and the second shows the chip.
    let with_link = |body: &str| {
        let mut app = app_reading(body);
        let mut view = crate::tui::ui::ViewState::default();
        render_backend(120, 20, |frame| draw_app(frame, &mut app, &mut view));
        app.reader_hints
            .sync(view.reader.hints.clone(), view.reader.openable);
        render_text(app, 120, 20)
    };

    assert!(with_link("See [one](https://example.com).").contains("open link"));
    assert!(!with_link("Just prose, no links at all.").contains("open link"));
}
