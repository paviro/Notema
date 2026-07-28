//! Flat (bg-layered) versus bordered chrome across every widget: dialogs,
//! panels, journal and entry cards, scrollbars, footers, toasts, and notices.

use super::*;
use crate::tui::state::{HoverTarget, MetadataKind};

/// An app whose `work` journal holds `count` entries, each with a distinct tag, so
/// the filter browser's Tags tab overflows its visible window and shows a scrollbar.
fn app_with_many_tags(count: usize) -> AppModel {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    for index in 0..count {
        fs::write(
            entry_dir.join(format!("{index}.md")),
            format!(
                "+++\nschema_version = 1\n\n[entry]\ntags = [\"tag-{index:02}\"]\n\n[time]\ncreated_at = \"2026-07-01T10:{index:02}:00+02:00\"\n+++\n\n# Entry {index}\nBody\n"
            ),
        )
        .unwrap();
    }
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    std::mem::forget(dir);
    app
}

fn tags_state() -> EditMetadataState {
    EditMetadataState::new(
        MetadataKind::Tags,
        vec![("work".to_string(), 3), ("home".to_string(), 1)],
        vec![0, 1],
        Vec::new(),
        2,
    )
}

fn many_tags_state() -> EditMetadataState {
    let all_values: Vec<_> = (0..20)
        .map(|index| (format!("tag-{index:02}"), index))
        .collect();
    let filtered = (0..all_values.len()).collect();
    EditMetadataState::new(MetadataKind::Tags, all_values, filtered, Vec::new(), 20)
}

#[test]
fn dialogs_drop_borders_for_a_title_row_with_esc_hint() {
    let rendered = render_edit_tags_dialog_text_with_theme(&flat_theme(), tags_state(), 80, 24);
    assert!(!rendered.contains('┌'), "flat dialog still draws corners");
    assert!(
        !rendered.contains('│'),
        "flat dialog still draws side borders"
    );
    assert!(rendered.contains("Edit Tags"));
    assert!(rendered.contains("esc"));
}

#[test]
fn dialog_surface_carries_the_dialog_background() {
    let theme = flat_theme();
    let dialog_bg = theme.dialog_bg();
    let backend = render_backend(80, 24, |frame| {
        dialogs::draw_edit_metadata_dialog(
            &theme,
            frame,
            &mut tags_state(),
            crate::tui::state::HoverTarget::None,
        )
    });
    let area = metadata_dialog_layout(&theme, Rect::new(0, 0, 80, 24), 2).area;
    let cell = &backend.buffer()[(area.x + 1, area.y + 1)];
    assert_eq!(cell.bg, dialog_bg);
}

#[test]
fn bordered_dialog_list_keeps_one_cell_before_the_frame_and_scrollbar() {
    let theme = flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
    let frame_area = Rect::new(0, 0, 80, 20);
    let layout = metadata_dialog_layout(&theme, frame_area, 20);
    let backend = render_backend(frame_area.width, frame_area.height, |frame| {
        dialogs::draw_edit_metadata_dialog(&theme, frame, &mut many_tags_state(), HoverTarget::None)
    });
    let bar = scrollbar_bar_rect(&theme, layout.area);
    let row = layout.list.y;

    assert_eq!(layout.list.x, layout.area.x + 2);
    assert_eq!(layout.list.x + layout.list.width, bar.x - 1);
    assert_eq!(backend.buffer()[(layout.area.x + 1, row)].symbol(), " ");
    assert_eq!(backend.buffer()[(bar.x - 1, row)].symbol(), " ");
    assert_ne!(backend.buffer()[(bar.x, row)].symbol(), " ");
}

#[test]
fn flat_dialog_widens_non_list_rows_flush_with_the_scrollbar() {
    let theme = flat_theme();
    let frame_area = Rect::new(0, 0, 60, 20);
    let layout = metadata_dialog_layout(&theme, frame_area, 20);
    let backend = render_backend(frame_area.width, frame_area.height, |frame| {
        dialogs::draw_edit_metadata_dialog(&theme, frame, &mut many_tags_state(), HoverTarget::None)
    });
    // The bar sits one padding column past the list; non-list rows run flush
    // with its right edge, and the list stays two columns narrower.
    let bar_x = layout.list.x + layout.list.width + 1;
    assert_eq!(layout.inner.width, layout.list.width + 2);
    assert_eq!(layout.inner.x + layout.inner.width, bar_x + 1);

    // The surface margin is symmetric: the content's left inset equals the
    // blank margin to the right of the bar.
    let left_margin = layout.inner.x - layout.area.x;
    let right_margin = (layout.area.x + layout.area.width) - (bar_x + 1);
    assert_eq!(left_margin, right_margin);

    // The separator paints up to the bar column; the list row leaves its
    // padding column blank, and the right margin columns stay blank.
    assert_ne!(
        backend.buffer()[(bar_x, layout.list_top_separator.y)].symbol(),
        " "
    );
    assert_eq!(backend.buffer()[(bar_x - 1, layout.list.y)].symbol(), " ");
    assert_eq!(backend.buffer()[(bar_x + 1, layout.list.y)].symbol(), " ");
}

#[test]
fn flat_dialog_list_fills_full_width_when_it_does_not_overflow() {
    let theme = flat_theme();
    let frame_area = Rect::new(0, 0, 60, 20);
    // Two tags in a 20-row dialog never overflow, so no bar is drawn and the
    // list reclaims the gutter to run flush with the other rows.
    let layout = metadata_dialog_layout(&theme, frame_area, 2);
    assert!(layout.list.height as usize >= 2);
    assert_eq!(layout.list.width, layout.inner.width);

    let bar_x = layout.list.x + layout.list.width + 1;
    let backend = render_backend(frame_area.width, frame_area.height, |frame| {
        dialogs::draw_edit_metadata_dialog(&theme, frame, &mut tags_state(), HoverTarget::None)
    });
    // No scrollbar column past the flush list.
    assert_eq!(backend.buffer()[(bar_x, layout.list.y)].symbol(), " ");
}

#[test]
fn bordered_dialog_keeps_non_list_rows_at_the_list_width() {
    let theme = flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
    let layout = metadata_dialog_layout(&theme, Rect::new(0, 0, 60, 20), 20);
    // Bordered chrome reserves nothing beside the list, so every row shares
    // one width and the bar rides the frame border.
    assert_eq!(layout.inner.width, layout.list.width);
}

#[test]
fn flat_filter_dialog_scrollbar_scopes_to_the_list_rows() {
    let frame_area = Rect::new(0, 0, 90, 24);
    let mut app = flat_app(app_with_many_tags(20));
    app.nav.focus = Focus::Journals;
    app.begin_filter();
    let theme = app.appearance.theme.clone();

    let layout = filter_dialog_layout(&theme, frame_area, app.filter_state().unwrap());
    // The Tags tab overflows its window, so the list narrows by the scrollbar
    // gutter while the tab strip and hint rows run flush with the bar's edge —
    // the same list-scoped treatment as every other dialog.
    assert!(layout.list.height < 20);
    assert_eq!(layout.hints.width, layout.list.width + 2);
    assert_eq!(layout.tabs.width, layout.hints.width);

    let bar_x = layout.list.x + layout.list.width + 1;
    assert_eq!(layout.hints.x + layout.hints.width, bar_x + 1);

    let backend = render_backend(frame_area.width, frame_area.height, |frame| {
        dialogs::draw_filter_dialog(
            &theme,
            frame,
            app.filter_state_mut().unwrap(),
            HoverTarget::None,
        )
    });
    let glyphs = theme.glyphs();
    let scrollbar_glyphs = [
        glyphs.scrollbar_thumb,
        glyphs.scrollbar_track,
        glyphs.scrollbar_up,
        glyphs.scrollbar_down,
    ];
    let is_scrollbar = |x: u16, y: u16| {
        backend.buffer()[(x, y)]
            .symbol()
            .chars()
            .next()
            .is_some_and(|c| scrollbar_glyphs.contains(&c))
    };
    // The bar paints on the list rows but not on the tab strip or hint row.
    assert!(is_scrollbar(bar_x, layout.list.y));
    assert!(!is_scrollbar(bar_x, layout.tabs.y));
    assert!(!is_scrollbar(bar_x, layout.hints.y));
}

/// The help overlay is a dialog like the rest, so its bar rides the same
/// column and stays on the table's rows, rather than spanning the whole box at
/// the outer edge, crossing the tab strip and the hint block.
#[test]
fn help_dialog_scrollbar_matches_the_other_dialogs() {
    use crate::tui::state::HelpTab;

    for theme in [
        flat_theme().with_chrome_override(Some(theme::ChromeStyle::Flat)),
        flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered)),
    ] {
        // Short enough that the shortcut table has to scroll.
        let frame_area = Rect::new(0, 0, 90, 20);
        let layout = menus::help_dialog_layout(&theme, frame_area, HelpTab::Shortcuts);
        let tabs = layout.tabs.expect("the help dialog has tabs");
        let bar_x = layout.track.x + layout.track.width + 1;

        let backend = render_backend(frame_area.width, frame_area.height, |frame| {
            menus::draw_help(&theme, frame, HelpTab::Shortcuts, HoverTarget::None, &mut 0)
        });
        let glyphs = theme.glyphs();
        let scrollbar_glyphs = [
            glyphs.scrollbar_thumb,
            glyphs.scrollbar_track,
            glyphs.scrollbar_up,
            glyphs.scrollbar_down,
        ];
        let is_scrollbar = |x: u16, y: u16| {
            backend.buffer()[(x, y)]
                .symbol()
                .chars()
                .next()
                .is_some_and(|c| scrollbar_glyphs.contains(&c))
        };
        assert!(
            is_scrollbar(bar_x, layout.hints.y - 2),
            "the bar paints on the table's rows"
        );
        assert!(!is_scrollbar(bar_x, tabs.y), "not on the tab strip");
        assert!(!is_scrollbar(bar_x, layout.hints.y), "not on the hints");
    }
}

#[test]
fn dialog_scrollbar_spans_only_the_list_rows() {
    let theme = flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
    let frame_area = Rect::new(0, 0, 80, 20);
    let layout = metadata_dialog_layout(&theme, frame_area, 20);
    let backend = render_backend(frame_area.width, frame_area.height, |frame| {
        dialogs::draw_edit_metadata_dialog(&theme, frame, &mut many_tags_state(), HoverTarget::None)
    });
    let bar = scrollbar_bar_rect(&theme, layout.area);
    let glyphs = theme.glyphs();
    let scrollbar_glyphs = [
        glyphs.scrollbar_thumb,
        glyphs.scrollbar_track,
        glyphs.scrollbar_up,
        glyphs.scrollbar_down,
    ];
    let is_scrollbar = |x: u16, y: u16| {
        backend.buffer()[(x, y)]
            .symbol()
            .chars()
            .next()
            .is_some_and(|c| scrollbar_glyphs.contains(&c))
    };

    // The list rows carry the scrollbar; the chrome above and below it does
    // not — the bordered frame stays a plain border there.
    assert!(is_scrollbar(bar.x, layout.list.y));
    assert!(is_scrollbar(bar.x, layout.list.y + layout.list.height - 1));
    assert!(!is_scrollbar(bar.x, layout.list.y.saturating_sub(1)));
    assert!(!is_scrollbar(bar.x, layout.list.y + layout.list.height));
    assert!(!is_scrollbar(bar.x, layout.hints.y));
}

#[test]
fn selection_is_a_bg_fill_not_reversed() {
    let theme = flat_theme();
    let selection = theme.selection();
    let layout = metadata_dialog_layout(&theme, Rect::new(0, 0, 80, 24), 2);
    let backend = render_backend(80, 24, |frame| {
        dialogs::draw_edit_metadata_dialog(
            &theme,
            frame,
            &mut tags_state(),
            crate::tui::state::HoverTarget::None,
        )
    });
    let row = layout.list.y;
    let cell = &backend.buffer()[(layout.list.x + 3, row)];
    assert_eq!(cell.bg, selection.bg.unwrap());
    assert!(!cell.modifier.contains(Modifier::REVERSED));
}

#[test]
fn focused_panel_gets_a_stripe_instead_of_a_thick_border() {
    let app = flat_app(app_with_journals(&["alpha", "beta"]));
    let backend = render_app(app, 120, 30);
    let rendered: String = backend
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(rendered.contains('┃'), "focus stripe missing");
    // Only the panel borders (thick when focused) must be gone.
    assert!(
        !rendered.contains('┏'),
        "thick border corner leaked into flat chrome"
    );
}

#[test]
fn journal_cards_keep_a_uniform_row_geometry() {
    let app = flat_app(app_with_journals(&["work", "zeta", "old.archived"]));
    let rows = crate::tui::entry_rows::journal_list_rows(&app, 16);
    let meta = crate::tui::entry_rows::rows_meta(&rows);
    // Same shape as the bordered column, one separator row taller: uniform
    // rows (divider included) so scroll and hit-testing stay a multiply.
    let indices: Vec<Option<usize>> = meta.iter().map(|m| m.item_index).collect();
    assert_eq!(indices, vec![Some(0), Some(1), None, Some(2)]);
    assert!(
        meta.iter()
            .all(|m| { m.height == crate::tui::render::journal_row_height(&app.appearance.theme) })
    );
}

#[test]
fn journal_cards_carry_selection_and_element_backgrounds() {
    let theme = flat_theme();
    let app = flat_app(app_with_journals(&["work", "zeta"]));
    let layout = tui_layout(Rect::new(0, 0, 120, 30), &app);
    let journals = layout.journals.unwrap();
    let list = journal_list_rect(journals.content);
    let buffer_backend = render_app(app, 120, 30);
    let buffer = buffer_backend.buffer();

    // Cards fill three rows (padding, name, padding); the fourth is the gap.
    let selection_bg = theme.selection().bg.unwrap();
    for y in [list.y, list.y + 1, list.y + 2] {
        assert_eq!(
            buffer[(list.x + 2, y)].bg,
            selection_bg,
            "selected card row {y} misses the selection background"
        );
    }
    let gap = &buffer[(list.x + 2, list.y + 3)];
    assert_ne!(gap.bg, selection_bg, "separator row painted like the card");
    let unselected = &buffer[(list.x + 2, list.y + 5)];
    assert_eq!(unselected.bg, theme.raised_bg());

    // No box-drawing left in the journal column.
    for y in journals.content.y..journals.content.bottom() {
        for x in journals.content.x..journals.content.right() {
            let symbol = buffer[(x, y)].symbol();
            assert!(
                !"┌┐└┘".contains(symbol),
                "box corner {symbol:?} at ({x},{y}) in flat journal column"
            );
        }
    }
}

#[test]
fn all_journals_search_floods_every_card_but_not_the_divider() {
    let theme = flat_theme();
    let mut app = flat_app(app_with_journals(&["work", "zeta", "old.archived"]));
    app.nav.mode = crate::tui::app::Mode::Search;
    app.search.scope = crate::tui::app::SearchScope::AllJournals;

    // ≥ INLINE_READER_MIN_WIDTH so the journal column stays visible in
    // search mode.
    let layout = tui_layout(Rect::new(0, 0, 140, 30), &app);
    let journals = layout.journals.unwrap();
    let list = journal_list_rect(journals.content);
    let backend = render_app(app, 140, 30);
    let buffer = backend.buffer();

    let selection_bg = theme.selection().bg.unwrap();
    // Card name rows at 1, 5 (active) and 13 (archived, after the divider
    // block at rows 8..12).
    for card_y in [list.y + 1, list.y + 5, list.y + 13] {
        assert_eq!(
            buffer[(list.x + 2, card_y)].bg,
            selection_bg,
            "card at row {card_y} not flooded by the all-journals search"
        );
    }
    let divider = &buffer[(list.x + 2, list.y + 9)];
    assert_ne!(divider.bg, selection_bg, "divider flooded");
}

#[test]
fn hovered_journal_card_lifts_to_the_hover_background() {
    let theme = flat_theme();
    let mut app = flat_app(app_with_journals(&["work", "zeta"]));
    app.hover = crate::tui::state::HoverTarget::Journal(1);
    let layout = tui_layout(Rect::new(0, 0, 120, 30), &app);
    let list = journal_list_rect(layout.journals.unwrap().content);
    let backend = render_app(app, 120, 30);
    let buffer = backend.buffer();
    // The second (unselected) card sits one row block down; hovered, its
    // background is the hover lift instead of the element surface.
    let hovered = &buffer[(list.x + 2, list.y + 5)];
    assert_eq!(hovered.bg, theme.hover().bg.unwrap());
    // The selected card keeps its selection background.
    let selected = &buffer[(list.x + 2, list.y + 1)];
    assert_eq!(selected.bg, theme.selection().bg.unwrap());
}

#[test]
fn entry_cards_sit_on_the_element_surface_with_plain_spacers() {
    let theme = flat_theme();
    let mut app = flat_app(app_with_entries(2));
    app.nav.selected_entry_index = None;
    let layout = tui_layout(Rect::new(0, 0, 120, 30), &app);
    let content = layout.entries.unwrap().panel.content;
    let backend = render_app(app, 120, 30);
    let buffer = backend.buffer();

    // Row 0 is the leading blank; the first card starts on row 1.
    let card = &buffer[(content.x + 1, content.y + 1)];
    assert_eq!(card.bg, theme.raised_bg());
    // Spacer rows keep the panel surface so the cards read as blocks.
    let spacer = &buffer[(content.x + 1, content.y)];
    assert_ne!(spacer.bg, theme.raised_bg());
}

#[test]
fn hovered_entry_box_carries_the_hover_background() {
    let theme = flat_theme();
    let mut app = flat_app(app_with_entry());
    // Deselect so the selection highlight doesn't own the hovered row.
    app.nav.selected_entry_index = None;
    app.hover = crate::tui::state::HoverTarget::Entry(0);
    let layout = tui_layout(Rect::new(0, 0, 120, 30), &app);
    let content = layout.entries.unwrap().panel.content;
    let backend = render_app(app, 120, 30);
    let buffer = backend.buffer();
    // Row 0 is the leading blank; the entry's box starts on row 1.
    let cell = &buffer[(content.x + 1, content.y + 1)];
    assert_eq!(cell.bg, theme.hover().bg.unwrap());
}

#[test]
fn hovered_insights_tab_uses_hint_style_text_without_hover_background() {
    let theme = flat_theme();
    let mut app = flat_app(app_with_entry());
    focus_insights(&mut app, InsightsTab::Overview);
    app.hover = HoverTarget::InsightsTab(InsightsTab::Writing);
    let layout = tui_layout(Rect::new(0, 0, 140, 30), &app);
    let insights = layout.insights.expect("insights panel");
    let col = (insights.area.x..insights.area.x + insights.area.width)
        .find(|col| {
            insights_tab_at(&theme, insights.area, *col, insights.area.y)
                == Some(InsightsTab::Writing)
        })
        .expect("writing tab");

    let backend = render_app(app, 140, 30);
    let cell = &backend.buffer()[(col, insights.area.y)];
    assert_eq!(cell.fg, theme.text().fg.unwrap());
    assert_ne!(cell.bg, theme.hover().bg.unwrap());
}

#[test]
fn focused_insights_active_tab_uses_the_accent_title_style_not_a_fill() {
    let theme = flat_theme();
    let mut app = flat_app(app_with_entry());
    focus_insights(&mut app, InsightsTab::Overview);
    let layout = tui_layout(Rect::new(0, 0, 140, 30), &app);
    let insights = layout.insights.expect("insights panel");
    let col = (insights.area.x..insights.area.x + insights.area.width)
        .find(|col| {
            insights_tab_at(&theme, insights.area, *col, insights.area.y)
                == Some(InsightsTab::Overview)
        })
        .expect("overview tab");

    let backend = render_app(app, 140, 30);
    let cell = &backend.buffer()[(col, insights.area.y)];
    // The active tab is styled with the `secondary` accent (see theme docs);
    // it only equalled `primary` back when the default theme left secondary
    // inheriting primary.
    assert_eq!(cell.fg, theme.secondary().fg.unwrap());
    assert_ne!(cell.bg, theme.selection().bg.unwrap());
}

#[test]
fn entry_cards_embed_the_border_labels_inside_padding() {
    let flat_theme = flat_theme();
    let flat = rendered_lines(&entry_box_lines(
        &flat_theme,
        Some("Sunday 05"),
        "14:30",
        "hello world",
        Some("2 words ★"),
        Some("Archived"),
        40,
    ));
    let bordered_theme = theme::Theme::terminal_default();
    let bordered = rendered_lines(&entry_box_lines(
        &bordered_theme,
        Some("Sunday 05"),
        "14:30",
        "hello world",
        Some("2 words ★"),
        Some("Archived"),
        40,
    ));

    // The flat card pads one blank row above the header and below the
    // footer so the labels sit off the card's edge; heights are per-row
    // metadata, so differing from the bordered box is fine.
    assert_eq!(flat.len(), bordered.len() + 2);
    assert_eq!(flat.first().unwrap().trim(), "");
    assert_eq!(flat.last().unwrap().trim(), "");

    // No border glyphs anywhere in the card.
    for line in &flat {
        assert!(
            !line.contains(['┌', '┐', '└', '┘', '│', '─']),
            "border glyph left in flat card line {line:?}"
        );
    }
    // The border labels move into the header and footer rows.
    assert!(flat[1].starts_with("  Sunday 05"));
    assert!(flat[1].trim_end().ends_with("14:30"));
    let footer = &flat[flat.len() - 2];
    assert!(footer.starts_with("  2 words ★"));
    assert!(footer.trim_end().ends_with("Archived"));
}

#[test]
fn bordered_dialogs_on_colored_themes_carry_the_dialog_surface() {
    // A colored theme forced into bordered chrome must not fall back to
    // the terminal-default background inside its dialogs (`Clear` alone
    // would). Classic is unaffected: its panel is the terminal default.
    let theme = flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
    let dialog_bg = theme.dialog_bg();
    let backend = render_backend(80, 24, |frame| {
        dialogs::draw_edit_metadata_dialog(
            &theme,
            frame,
            &mut tags_state(),
            crate::tui::state::HoverTarget::None,
        )
    });
    let area = metadata_dialog_layout(&theme, Rect::new(0, 0, 80, 24), 2).area;
    let border = &backend.buffer()[(area.x, area.y)];
    assert_eq!(border.symbol(), "┌", "chrome override not applied");
    assert_eq!(
        border.fg,
        theme.dialog_border().fg.unwrap(),
        "dialog frame fell back to terminal-default ink"
    );
    let interior = &backend.buffer()[(area.x + 1, area.y + 1)];
    assert_eq!(interior.bg, dialog_bg);
}

#[test]
fn dialogs_repaint_the_theme_ink_after_clearing() {
    // `Clear` resets cells to the terminal's own colors; the dialog frame
    // must re-establish the theme's text fg along with its surface, or
    // unstyled dialog text renders in the terminal's ink — near-white on a
    // light-mode dialog on a dark terminal.
    for chrome in [
        None,
        Some(crate::tui::theme::ChromeStyle::Flat),
        Some(crate::tui::theme::ChromeStyle::Bordered),
    ] {
        let theme = flat_theme().with_chrome_override(chrome);
        let ink = theme.text().fg.expect("flat theme has body ink");
        let backend = render_backend(80, 24, |frame| {
            frames::draw_dialog_frame(&theme, frame, Rect::new(10, 5, 40, 10), "Title", false);
        });
        let interior = &backend.buffer()[(12, 10)];
        assert_eq!(
            interior.fg, ink,
            "dialog interior lost the theme ink ({chrome:?})"
        );
    }
}

#[test]
fn bordered_key_chips_carry_the_key_hint_token() {
    // The key_hint token defaults to the classic REVERSED|BOLD chip, so
    // classic is a no-op — but a flat theme forced to bordered must keep its
    // own key_hint ink.
    let theme = flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
    let mut app = app_with_journals(&["alpha"]);
    app.appearance.theme = theme.clone();
    let text = footer::footer_lines(&app.appearance.theme, &app, 120);
    let spans: Vec<_> = text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .collect();
    let label = spans
        .iter()
        .position(|span| span.content == " quit")
        .expect("quit label in the browse footer");
    let chip = spans[label - 1];
    assert_eq!(chip.style, theme.key_hint(), "chip ignored the token");
}

#[test]
fn scrollbars_carry_the_scrollbar_tokens_on_both_chromes() {
    use ratatui::widgets::ScrollbarState;
    for (chrome_override, scrollbar_x) in [
        (crate::tui::theme::ChromeStyle::Flat, 2),
        (crate::tui::theme::ChromeStyle::Bordered, 3),
    ] {
        let theme = flat_theme().with_chrome_override(Some(chrome_override));
        let backend = render_backend(4, 12, |frame| {
            let mut state = ScrollbarState::default()
                .content_length(100)
                .viewport_content_length(10)
                .position(0);
            chrome::render_vertical_scrollbar(&theme, frame, frame.area(), &mut state, true);
        });
        // One vertical margin row and an arrow on each end; the thumb hugs
        // the top at position 0 and the track fills the rest.
        let thumb = &backend.buffer()[(scrollbar_x, 2u16)];
        let track = &backend.buffer()[(scrollbar_x, 9u16)];
        assert_eq!(thumb.fg, theme.scrollbar_thumb(true).fg.unwrap());
        assert_eq!(track.fg, theme.scrollbar_track(true).fg.unwrap());
    }
}

#[test]
fn themed_border_set_draws_panels_cards_and_tables() {
    let rounded = theme::test_theme_from_toml("[borders]\nstyle = \"rounded\"");
    let corner = |focused: bool| {
        let active_theme = rounded.clone();
        let backend = render_backend(20, 5, move |frame| {
            frame.render_widget(
                chrome::panel_block(&active_theme, "t", focused, None),
                frame.area(),
            );
        });
        backend.buffer()[(0u16, 0u16)].symbol().to_string()
    };
    assert_eq!(corner(false), "╭", "unfocused panel ignored the set");
    assert_eq!(corner(true), "┏", "focus must stay thick");

    let ascii = theme::test_theme_from_toml("[borders]\nstyle = \"ascii\"");
    let text = |line: ratatui::text::Line| -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    };
    assert_eq!(
        text(crate::tui::entry_rows::border_line(
            &ascii,
            crate::tui::entry_rows::BoxEdge::Top,
            10,
            None,
            None,
        )),
        "+--------+"
    );
    assert_eq!(
        text(table::themed_rule(
            &ascii,
            &[3],
            table::RulePos::Top,
            ratatui::style::Style::default(),
            ratatui::style::Style::default(),
        )),
        "+-----+"
    );
}

#[test]
fn bordered_chrome_styles_unfocused_panel_borders_with_the_theme() {
    // A flat-designed theme forced into bordered chrome must not draw
    // inactive panel borders in the terminal-default ink — that reads
    // *brighter* than the focused border on a muted palette. Classic is
    // unaffected: its `border_inactive` is the terminal default.
    let theme = flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
    let render_theme = theme.clone();
    let corner = |focused: bool| {
        let active_theme = render_theme.clone();
        let backend = render_backend(20, 5, move |frame| {
            frame.render_widget(
                chrome::panel_block(&active_theme, "t", focused, None),
                frame.area(),
            );
        });
        backend.buffer()[(0u16, 0u16)].clone()
    };
    assert_eq!(corner(false).fg, theme.inactive_border().fg.unwrap());
    assert_eq!(corner(true).fg, theme.focus_border().fg.unwrap());
}

#[test]
fn hovered_dialog_row_lifts_even_when_it_is_the_hidden_selection() {
    let theme = flat_theme();
    // Focus on the input: the list's selection highlight is hidden, so the
    // selected row (index 0, the default) must still respond to hover.
    let mut state = tags_state();
    state.focus = EditMetadataFocus::Input;
    let layout = metadata_dialog_layout(&theme, Rect::new(0, 0, 80, 24), 2);
    let backend = render_backend(80, 24, |frame| {
        dialogs::draw_edit_metadata_dialog(
            &theme,
            frame,
            &mut state,
            crate::tui::state::HoverTarget::DialogRow(0),
        )
    });
    let cell = &backend.buffer()[(layout.list.x + 3, layout.list.y)];
    assert_eq!(cell.bg, theme.hover().bg.unwrap());
}

#[test]
fn hovered_footer_hint_label_lifts_out_of_the_muted_row() {
    let theme = flat_theme();
    let mut app = flat_app(app_with_journals(&["alpha"]));
    app.hover = crate::tui::state::HoverTarget::FooterHint(footer::HintId::Quit);
    let text = footer::footer_lines(&app.appearance.theme, &app, 120);
    let label = text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content == " quit")
        .expect("quit label in the browse footer");
    assert_eq!(label.style, theme.text(), "hovered label still muted");
}

#[test]
fn bordered_footer_hint_labels_keep_flat_text_styles() {
    let theme = flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
    let mut app = app_with_journals(&["alpha"]);
    app.appearance.theme = theme.clone();
    let label_style = |app: &AppModel| {
        footer::footer_lines(&app.appearance.theme, app, 120)
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == " quit")
            .expect("quit label in the browse footer")
            .style
    };

    assert_eq!(label_style(&app), theme.muted());
    app.hover = crate::tui::state::HoverTarget::FooterHint(footer::HintId::Quit);
    assert_eq!(label_style(&app), theme.text());
}

#[test]
fn footer_key_chips_are_not_reversed() {
    let app = flat_app(app_with_journals(&["alpha"]));
    let backend = render_app(app, 120, 30);
    let buffer = backend.buffer();
    for y in 0..30u16 {
        for x in 0..120u16 {
            assert!(
                !buffer[(x, y)].modifier.contains(Modifier::REVERSED),
                "reversed cell at ({x},{y}) in flat chrome"
            );
        }
    }
}

#[test]
fn flat_dialogs_pad_the_title_and_footer_off_the_card_edge() {
    let theme = flat_theme();
    // Regular dialog: blank row, then the title row.
    let backend = render_backend(80, 24, |frame| {
        dialogs::draw_edit_metadata_dialog(
            &theme,
            frame,
            &mut tags_state(),
            crate::tui::state::HoverTarget::None,
        )
    });
    let area = metadata_dialog_layout(&theme, Rect::new(0, 0, 80, 24), 2).area;
    let row_text = |y: u16| -> String {
        (area.x..area.x + area.width)
            .map(|x| backend.buffer()[(x, y)].symbol())
            .collect()
    };
    assert_eq!(row_text(area.y).trim(), "", "top padding row not blank");
    assert!(row_text(area.y + 1).contains("Edit Tags"));
    assert_eq!(
        row_text(area.y + 2).trim(),
        "",
        "no blank row under the title"
    );
    assert_eq!(
        row_text(area.y + area.height - 1).trim(),
        "",
        "bottom padding row not blank"
    );
}

#[test]
fn dialog_inner_uses_the_shared_surface_gutter_in_both_chromes() {
    let area = Rect::new(10, 5, 44, 20);
    for (chrome, expected) in [
        (
            crate::tui::theme::ChromeStyle::Flat,
            Rect::new(12, 8, 39, 16),
        ),
        (
            crate::tui::theme::ChromeStyle::Bordered,
            Rect::new(12, 6, 40, 18),
        ),
    ] {
        let theme = flat_theme().with_chrome_override(Some(chrome));
        let inner = frames::dialog_inner(&theme, area);
        assert_eq!(inner, expected);
        assert_eq!(inner.x, area.x + 2);
        assert_eq!(
            inner.x + inner.width,
            scrollbar_bar_rect(&theme, area).x - 1
        );
    }
}
