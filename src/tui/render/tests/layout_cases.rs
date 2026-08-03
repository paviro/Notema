//! Where the columns and panels land at each width, and the interaction
//! regions rendering registers for them.

use super::*;

#[test]
fn layout_places_hit_targets_in_three_columns() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;

    let layout = tui_layout(Rect::new(0, 0, 140, 20), &app);

    assert!(!layout.single_panel);
    assert!(layout.reader.is_some());
    assert!(layout.insights.is_none());
    // The three columns share the rows the footer doesn't take.
    let footer_h = footer_height(&app, 140);
    let content_h = 20 - footer_h;
    assert_eq!(
        layout.journals.unwrap().area,
        Rect::new(0, 0, 27, content_h)
    );
    assert_eq!(
        layout.entries.unwrap().panel.area,
        Rect::new(27, 0, 47, content_h)
    );
    assert_eq!(layout.reader.unwrap().area, Rect::new(74, 0, 66, content_h));
    assert_eq!(layout.footer, Rect::new(0, content_h, 140, footer_h));
}

#[test]
fn layout_keeps_three_columns_at_minimum_inline_width() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;

    let layout = tui_layout(Rect::new(0, 0, INLINE_READER_MIN_WIDTH, 20), &app);

    assert!(!layout.single_panel);
    assert!(layout.reader.is_some());
    assert!(layout.insights.is_none());
    let ch = 20 - footer_height(&app, INLINE_READER_MIN_WIDTH);
    assert_eq!(layout.journals.unwrap().area, Rect::new(0, 0, 27, ch));
    assert_eq!(layout.entries.unwrap().panel.area, Rect::new(27, 0, 47, ch));
    assert_eq!(layout.reader.unwrap().area, Rect::new(74, 0, 51, ch));
}

#[test]
fn layout_places_hit_targets_in_two_columns_without_inline_reader() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Journals;

    let layout = tui_layout(Rect::new(0, 0, 90, 20), &app);

    assert!(!layout.single_panel);
    assert!(layout.reader.is_none());
    assert!(layout.insights.is_none());
    let ch = 20 - footer_height(&app, 90);
    assert_eq!(layout.journals.unwrap().area, Rect::new(0, 0, 27, ch));
    assert_eq!(layout.entries.unwrap().panel.area, Rect::new(27, 0, 63, ch));
}

#[test]
fn layout_shifts_two_columns_to_entries_and_reader_when_entries_are_active() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;

    let layout = tui_layout(Rect::new(0, 0, 90, 20), &app);

    assert!(!layout.single_panel);
    assert!(layout.reader.is_some());
    assert!(layout.insights.is_none());
    assert!(layout.journals.is_none());
    let content_height = 20 - footer_height(&app, 90);
    assert_eq!(
        layout.entries.unwrap().panel.area,
        Rect::new(0, 0, 47, content_height)
    );
    assert_eq!(
        layout.reader.unwrap().area,
        Rect::new(47, 0, 43, content_height)
    );
}

#[test]
fn layout_uses_single_compact_panel_for_active_focus() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Journals;

    let journals = tui_layout(Rect::new(0, 0, 57, 20), &app);
    assert!(journals.single_panel);
    assert_eq!(
        journals.journals.unwrap().area,
        Rect::new(0, 0, 57, 20 - footer_height(&app, 57))
    );
    assert!(journals.entries.is_none());

    app.nav.focus = Focus::Entries;
    let entries = tui_layout(Rect::new(0, 0, 57, 20), &app);
    assert!(entries.single_panel);
    assert_eq!(
        entries.entries.unwrap().panel.area,
        Rect::new(0, 0, 57, 20 - footer_height(&app, 57))
    );
    assert!(entries.journals.is_none());
}

#[test]
fn entry_list_geometry_is_shared_by_render_hit_test_and_visibility() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;
    let layout = tui_layout(Rect::new(0, 0, 80, 20), &app);
    let entries = layout.entries.unwrap();

    assert_eq!(
        entries.text_width,
        entries.panel.content.width.saturating_sub(4)
    );

    let rows = entry_row_metadata(&app, entries.text_width);
    // Row 0 is the month divider; the single entry's box occupies rows 1-3.
    let click_y = entries.panel.content.y + 2;

    assert_eq!(
        entry_index_at(
            entries,
            entries.panel.content.x,
            click_y,
            app.nav.entry_list.offset(),
            &rows
        ),
        Some(0)
    );

    let offset_before = app.nav.entry_list.offset();
    app.entry_list_ensure_visible(&rows, entries.viewport_height);
    assert_eq!(app.nav.entry_list.offset(), offset_before);
}

#[test]
fn panel_content_rect_defines_selectable_rows_not_padding() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Journals;
    let layout = tui_layout(Rect::new(0, 0, 120, 20), &app);
    let journals = layout.journals.unwrap();
    let list_area = journal_list_rect(journals.content);
    let inner_width = list_area.width.saturating_sub(4) as usize;
    let rows = crate::tui::entry_rows::journal_list_rows(&app, inner_width);
    let meta = crate::tui::entry_rows::rows_meta(&rows);

    // The first journal box sits one row below the content top (the leading
    // offset that aligns it with the entry list's first box).
    assert_eq!(
        journal_index_at(
            journals.content,
            journals.content.x,
            journals.content.y + 1,
            app.nav.journal_list.offset(),
            &meta,
        ),
        Some(0)
    );
    assert_eq!(
        journal_index_at(
            journals.content,
            panel_inner(journals.area).x,
            panel_inner(journals.area).y,
            app.nav.journal_list.offset(),
            &meta,
        ),
        None
    );
}

#[test]
fn rendering_registers_visible_rows_above_their_panel() {
    let mut app = app_with_journals(&["work", "zeta"]);
    let area = Rect::new(0, 0, 120, 20);
    let journals = tui_layout(area, &app).journals.expect("journals panel");
    let list = journal_list_rect(journals.content);
    let mut view = crate::tui::ui::ViewState::default();

    render_backend(area.width, area.height, |frame| {
        draw_app(frame, &mut app, &mut view)
    });

    assert_eq!(
        view.interactions.hit(list.x + 1, list.y + 1),
        Some(&crate::tui::ui::InteractionKind::Row {
            panel: crate::tui::ui::interaction::PanelId::Journals,
            index: 0,
        })
    );
}

#[test]
fn rendering_registers_scrollbar_regions_only_when_content_overflows() {
    use crate::tui::ui::InteractionKind;

    // An overflowing entry list registers the bar's widened grab region,
    // carrying the same metrics the renderer drew the bar from.
    let mut app = app_with_entries(60);
    let area = Rect::new(0, 0, 120, 20);
    let entries = tui_layout(area, &app).entries.expect("entries panel");
    let bar = crate::tui::scroll::scrollbar_bar_rect(&app.appearance.theme, entries.panel.area);
    let mut view = crate::tui::ui::ViewState::default();
    render_backend(area.width, area.height, |frame| {
        draw_app(frame, &mut app, &mut view)
    });

    let hit = view.interactions.hit(bar.x, bar.y + 1);
    let Some(InteractionKind::Scrollbar(metrics)) = hit else {
        panic!("expected a scrollbar region on the bar, got {hit:?}");
    };
    assert_eq!(metrics.which, crate::tui::app::ScrollbarDrag::EntryList);
    assert_eq!(metrics.bar, bar);
    assert_eq!(
        metrics.max_scroll,
        metrics
            .content_length
            .saturating_sub(metrics.viewport as usize)
    );
    // The grab region spans one extra column on each side of the bar.
    assert!(matches!(
        view.interactions.hit(bar.x - 1, bar.y + 1),
        Some(InteractionKind::Scrollbar(_))
    ));
    assert!(matches!(
        view.interactions.hit(bar.x + 1, bar.y + 1),
        Some(InteractionKind::Scrollbar(_))
    ));

    // A list that fits registers no scrollbar region; the same point falls
    // through to the panel underneath.
    let mut app = app_with_entries(1);
    let mut view = crate::tui::ui::ViewState::default();
    render_backend(area.width, area.height, |frame| {
        draw_app(frame, &mut app, &mut view)
    });
    assert!(!matches!(
        view.interactions.hit(bar.x, bar.y + 1),
        Some(InteractionKind::Scrollbar(_))
    ));
}

#[test]
fn rendering_keeps_navigation_state_and_records_effective_scroll_in_the_view() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Reader;
    app.nav.scroll.reader = u16::MAX;
    *app.nav.journal_list.offset_mut() = usize::MAX;
    *app.nav.entry_list.offset_mut() = usize::MAX;
    let before = (
        app.nav.focus,
        app.nav.selected_entry_index,
        app.nav.scroll.reader,
        app.nav.reader_fullscreen,
        app.nav.journal_list.offset(),
        app.nav.entry_list.offset(),
    );
    let mut view = crate::tui::ui::ViewState::default();

    render_backend(140, 20, |frame| draw_app(frame, &mut app, &mut view));

    assert_eq!(
        before,
        (
            app.nav.focus,
            app.nav.selected_entry_index,
            app.nav.scroll.reader,
            app.nav.reader_fullscreen,
            app.nav.journal_list.offset(),
            app.nav.entry_list.offset(),
        )
    );
    assert!(view.reader.scroll < u16::MAX);
    assert_eq!(
        view.journals.as_ref().map(|journals| journals.offset),
        Some(0)
    );
    assert_eq!(view.entry_offset, Some(0));
}

#[test]
fn overlay_region_wins_over_underlying_rows() {
    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    let mut view = crate::tui::ui::ViewState::default();

    render_backend(64, 20, |frame| draw_app(frame, &mut app, &mut view));

    // A point in the backdrop margin, left of the centered dialog: the overlay's
    // full-frame region wins over the journal row registered behind it.
    assert_eq!(
        view.interactions.hit(2, 10),
        Some(&crate::tui::ui::InteractionKind::Overlay)
    );
}
