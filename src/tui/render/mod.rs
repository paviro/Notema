mod chrome;
mod dialogs;
mod editor;
mod entries;
mod footer;
mod frames;
mod hints;
mod image_viewer;
pub(crate) mod insights;
mod journals;
mod layout;
mod markdown;
mod menus;
mod metadata;
mod pending;
mod reader;
mod search_query;
pub(crate) mod tab_strip;
mod table;
mod toasts;
mod unlock;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    widgets::{ListState, Paragraph},
};

use super::app::AppModel;
use super::editor_state::EditorPrompt;
pub(crate) use super::entry_rows::RowMeta;
#[cfg(test)]
pub(crate) use super::entry_rows::entry_row_metadata;
#[cfg(test)]
pub(crate) use super::entry_rows::{
    entry_box_lines, entry_day_label, entry_list_lines, entry_month_label,
};
pub(crate) use super::hit_test::{
    MetadataChip, entry_index_at, journal_index_at, metadata_at_point, metadata_chip_index_at,
};
use super::scroll::scrollbar_bar_rect;
#[cfg(test)]
pub(crate) use super::scroll::scrollbar_position;
pub(crate) use super::scroll::{clamp_scroll, scroll_pixels, viewer_scroll};
#[cfg(test)]
use super::scroll::{scroll_from_thumb_top, scrollbar_thumb};
use super::state::ListNav;
#[cfg(test)]
pub(crate) use super::surface::panel_inner;
pub(crate) use super::surface::{
    EntryListGeometry, EntryMetadataValues, PanelGeometry, point_in_rect,
};
use super::ui::{
    ConfirmId, DialogId, DialogInputId, InteractionKind, RenderContext, TextFieldId,
    interaction::PanelId,
};
#[cfg(test)]
pub(crate) use chrome::dialog_list_scrollbar_rect;
pub(crate) use chrome::{
    centered_rect_fixed_size, container_block, container_block_vertical_inset, count_label,
    flat_chrome, panel_block, panel_focus_stripe, render_centered_notice,
    render_scrollbar_if_needed,
};
#[cfg(test)]
pub(crate) use dialogs::{
    confirm_delete_inner, feelings_dialog_hints, metadata_dialog_hints, mood_dialog_hints,
    mood_dialog_layout, theme_picker_hints,
};
use dialogs::{
    draw_confirm_delete, draw_edit_feelings_dialog, draw_edit_location_dialog,
    draw_edit_metadata_dialog, draw_edit_mood_dialog, draw_fetching_environment,
    draw_filter_dialog, draw_new_journal_input, draw_settings_dialog, draw_theme_picker,
};
pub(crate) use dialogs::{
    feelings_dialog_layout, filter_dialog_layout, location_dialog_layout, metadata_dialog_layout,
    settings_dialog_layout, theme_picker_layout,
};
use editor::draw_entry_editor;
use entries::draw_entry_list;
pub(crate) use entries::search_suggestions_list_rect;
pub(crate) use footer::{Hint, HintId, footer_height, footer_hint_id_at_point, footer_lines};
#[cfg(test)]
pub(crate) use footer::{
    footer_hint_id_at, footer_text, hint_grid_text, hint_height, hint_regions,
};
pub(crate) use frames::{draw_editor_discard_confirm, draw_modal_frame};
use image_viewer::draw_image_viewer;
use insights::draw_journal_insights;
pub(crate) use insights::insights_tab_at;
#[cfg(test)]
pub(crate) use journals::JOURNAL_BOX_HEIGHT;
use journals::draw_journals;
pub(crate) use journals::{journal_list_rect, journal_row_height};
#[cfg(test)]
use layout::metadata_scrolls_with_body;
pub(crate) use layout::{TuiLayout, tui_layout};
#[cfg(test)]
pub(crate) use menus::help_dialog_layout;
pub(crate) use menus::{draw_editor_shortcuts, draw_metadata_menu};
pub(crate) use pending::{
    AccessNotice, draw_disable_notice, draw_pending_notice, draw_pending_request,
};
use reader::draw_selected_reader;
#[cfg(test)]
pub(crate) use toasts::toast_rects;
pub(crate) use toasts::{countdown_cols, draw_toasts, toast_at_point};
pub(crate) use unlock::draw_unlock;

pub(crate) fn list_state_for_render(
    selected: Option<usize>,
    offset: usize,
    viewport_height: u16,
    highlight_active: bool,
) -> ListState {
    let visible_end = offset.saturating_add(viewport_height as usize);
    let visible_selection =
        selected.filter(|index| highlight_active && *index >= offset && *index < visible_end);
    ListState::default()
        .with_offset(offset)
        .with_selected(visible_selection)
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut AppModel, context: &mut RenderContext<'_>) {
    context.view.begin_frame();
    let theme = context.theme;
    let area = frame.area();
    let layout = tui_layout(area, app);
    context.view.layout = Some(layout);

    // Everything renders on the theme's background layer; a no-op for
    // terminal-default themes.
    frame
        .buffer_mut()
        .set_style(area, chrome::base_style(theme));

    if app.reader_is_fullscreen(area.width) {
        context
            .view
            .interactions
            .push(area, InteractionKind::Panel(PanelId::Reader));
        let footer_height = footer_height(app, area.width).min(area.height);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(footer_height)])
            .split(area);
        let body_layout = app.services.config.ui.layout.editor_body();
        if let Some(editor) = app.editor.as_mut() {
            draw_entry_editor(theme, frame, chunks[0], editor, body_layout);
        } else {
            draw_selected_reader(theme, frame, chunks[0], app, &mut context.view.reader);
        }
        let footer_area = chunks[1];
        frame
            .buffer_mut()
            .set_style(footer_area, chrome::footer_style(theme));
        frame.render_widget(
            Paragraph::new(footer_lines(theme, app, footer_area.width)),
            footer_area,
        );
        register_view_interactions(context, app, footer_area);
        if interaction_overlay_open(app) {
            context
                .view
                .interactions
                .push(area, InteractionKind::Overlay);
        }
        draw_overlays(theme, frame, app);
        register_overlay_interactions(context, app, area, None);
        draw_toasts(theme, frame, app);
        return;
    }

    if let Some(area) = layout.journals {
        context
            .view
            .interactions
            .push(area.area, InteractionKind::Panel(PanelId::Journals));
    }
    if let Some(area) = layout.entries {
        context
            .view
            .interactions
            .push(area.panel.area, InteractionKind::Panel(PanelId::Entries));
    }
    if let Some(area) = layout.reader {
        context
            .view
            .interactions
            .push(area.area, InteractionKind::Panel(PanelId::Reader));
    }
    if let Some(area) = layout.insights {
        context
            .view
            .interactions
            .push(area.area, InteractionKind::Panel(PanelId::Insights));
    }

    if let Some(area) = layout.journals {
        // Register rows against the clamped offset the rows were drawn with, not
        // the raw nav offset, so an over-max offset can't misplace click regions.
        let journals = draw_journals(theme, frame, area, app);
        register_rows(
            context,
            journals.list_area,
            &journals.meta,
            journals.offset,
            PanelId::Journals,
        );
        context.view.journals = Some(journals);
    }
    if let Some(area) = layout.entries {
        let offset = draw_entry_list(theme, frame, area, app);
        context.view.entry_offset = Some(offset);
        let rows = app.entry_rows(area.text_width);
        register_rows(
            context,
            area.panel.content,
            &rows.meta,
            offset,
            PanelId::Entries,
        );
    }
    if let Some(area) = layout.insights {
        draw_journal_insights(theme, frame, area.area, app, &mut context.view.insights);
    } else if let Some(area) = layout.reader {
        let body_layout = app.services.config.ui.layout.editor_body();
        if let Some(editor) = app.editor.as_mut() {
            draw_entry_editor(theme, frame, area.area, editor, body_layout);
        } else if app.show_journal_insights() {
            // With no entry selected, the reader pane shows the journal insights.
            draw_journal_insights(theme, frame, area.area, app, &mut context.view.insights);
        } else {
            draw_selected_reader(theme, frame, area.area, app, &mut context.view.reader);
        }
    }

    frame
        .buffer_mut()
        .set_style(layout.footer, chrome::footer_style(theme));
    let footer = Paragraph::new(footer_lines(theme, app, layout.footer.width));
    frame.render_widget(footer, layout.footer);
    register_view_interactions(context, app, layout.footer);

    if interaction_overlay_open(app) {
        context
            .view
            .interactions
            .push(area, InteractionKind::Overlay);
    }
    // After every panel, so the popup can dim what it hangs over; before the
    // overlays, so a real dialog still covers it.
    entries::draw_search_overlay(
        theme,
        frame,
        layout.entries.map(|entries| entries.panel.area),
        app,
    );
    draw_overlays(theme, frame, app);
    register_overlay_interactions(
        context,
        app,
        area,
        layout.entries.map(|entries| entries.panel.area),
    );
    draw_toasts(theme, frame, app);
}

/// Register a pane scrollbar's grab region from its panel area.
fn register_scrollbar(
    context: &mut RenderContext<'_>,
    which: crate::tui::app::ScrollbarDrag,
    panel_area: ratatui::layout::Rect,
    content_length: usize,
    viewport: u16,
    scroll: usize,
) {
    let bar = scrollbar_bar_rect(context.theme, panel_area);
    register_scrollbar_bar(context, which, bar, content_length, viewport, scroll);
}

/// Register a dialog list's scrollbar from the rect the list occupies, so the
/// grab region tracks `render_dialog_list_scrollbar`'s bar.
fn register_dialog_scrollbar(
    context: &mut RenderContext<'_>,
    dialog: DialogId,
    list: ratatui::layout::Rect,
    content_length: usize,
    scroll: usize,
) {
    let bar = chrome::dialog_list_scrollbar_rect(list);
    register_scrollbar_bar(
        context,
        crate::tui::app::ScrollbarDrag::Dialog(dialog),
        bar,
        content_length,
        list.height,
        scroll,
    );
}

/// Register a scrollbar's grab region — the one-cell bar plus one column on each
/// side, so it is easier to hit — carrying the metrics the mouse handler needs to
/// map presses and drags to scroll offsets. Skipped when the content fits, since
/// then no bar is drawn.
fn register_scrollbar_bar(
    context: &mut RenderContext<'_>,
    which: crate::tui::app::ScrollbarDrag,
    bar: ratatui::layout::Rect,
    content_length: usize,
    viewport: u16,
    scroll: usize,
) {
    let max_scroll = content_length.saturating_sub(viewport as usize);
    if max_scroll == 0 || bar.height == 0 {
        return;
    }
    let position = super::scroll::scrollbar_position(scroll, content_length, viewport);
    let left = bar.x.saturating_sub(1);
    let right = bar.x.saturating_add(1);
    let grab = ratatui::layout::Rect::new(left, bar.y, right - left + 1, bar.height);
    context.view.interactions.push(
        grab,
        InteractionKind::Scrollbar(super::ui::interaction::ScrollbarMetrics {
            which,
            bar,
            max_scroll,
            content_length,
            viewport,
            position,
        }),
    );
}

fn register_view_interactions(
    context: &mut RenderContext<'_>,
    app: &AppModel,
    footer: ratatui::layout::Rect,
) {
    use crate::tui::app::{Mode, ScrollbarDrag};

    // Scrollbars first: rows and panels are registered earlier in `draw`, so
    // these later pushes win the bar-adjacent columns (matching the old
    // probe-scrollbar-before-panels click order), while the reader link hits
    // pushed below keep beating the widened grab column.
    let layout = context.view.layout;
    if let Some(panel) = layout.and_then(|layout| layout.reader) {
        let (line_count, viewport, scroll) = {
            let reader = &context.view.reader;
            (reader.line_count, reader.content_rect.height, reader.scroll)
        };
        register_scrollbar(
            context,
            ScrollbarDrag::Reader,
            panel.area,
            line_count,
            viewport,
            scroll as usize,
        );
    }
    {
        let insights = &context.view.insights;
        let (insights_area, total, viewport, scroll) = (
            insights.area,
            insights.total,
            insights.viewport,
            insights.scroll,
        );
        register_scrollbar(
            context,
            ScrollbarDrag::Insights,
            insights_area,
            total,
            viewport,
            scroll as usize,
        );
    }
    if let Some(area) = layout.and_then(|layout| layout.entries) {
        let (total_height, scroll) = {
            let cache = app.entry_rows(area.text_width);
            let scroll = context
                .view
                .entry_offset
                .unwrap_or_else(|| app.nav.entry_list.offset());
            (cache.total_height, scroll)
        };
        register_scrollbar(
            context,
            ScrollbarDrag::EntryList,
            area.panel.area,
            total_height,
            area.viewport_height,
            scroll,
        );
    }
    if app.nav.mode == Mode::Browse
        && let Some(area) = layout.and_then(|layout| layout.journals)
        && let Some(journals) = context.view.journals.take()
    {
        let total_height = crate::tui::entry_rows::total_row_height(&journals.meta);
        register_scrollbar(
            context,
            ScrollbarDrag::Journals,
            area.area,
            total_height,
            journals.list_area.height,
            journals.offset,
        );
        context.view.journals = Some(journals);
    }

    let crate::tui::ui::ViewState {
        reader,
        interactions,
        ..
    } = &mut *context.view;
    for (hit, rect) in reader.visible_links() {
        interactions.push(
            rect,
            InteractionKind::Link {
                target: hit.target.clone(),
                heading_line: reader.heading_line_for(&hit.target),
            },
        );
    }

    for (row, start, width, id) in footer::footer_hint_regions(app, footer.width) {
        context.view.interactions.push(
            ratatui::layout::Rect::new(footer.x + start, footer.y + row, width, 1),
            InteractionKind::Hint(id),
        );
    }
}

fn register_overlay_interactions(
    context: &mut RenderContext<'_>,
    app: &AppModel,
    frame_area: ratatui::layout::Rect,
    entries_area: Option<ratatui::layout::Rect>,
) {
    use crate::tui::state::Overlay;

    match &app.overlay {
        Overlay::Settings(state) => {
            let layout = dialogs::settings_dialog_layout(context.theme, frame_area, state);
            // The whole list catches the wheel; only setting rows register a
            // clickable row, so a click on a sub-header is inert.
            context.view.interactions.push(
                layout.list,
                InteractionKind::DialogList {
                    dialog: DialogId::Settings,
                    viewport: layout.list.height,
                },
            );
            let offset = state.offset();
            for visible in 0..layout.list.height as usize {
                let index = offset.saturating_add(visible);
                let Some(item) = state.items.get(index) else {
                    break;
                };
                if !matches!(item, crate::tui::state::SettingsItem::Row { .. }) {
                    continue;
                }
                context.view.interactions.push(
                    ratatui::layout::Rect::new(
                        layout.list.x,
                        layout.list.y + visible as u16,
                        layout.list.width,
                        1,
                    ),
                    InteractionKind::DialogRow {
                        dialog: DialogId::Settings,
                        index,
                    },
                );
            }
            register_dialog_scrollbar(
                context,
                DialogId::Settings,
                layout.list,
                state.items.len(),
                offset,
            );
            register_hint_regions(context, layout.hints, dialogs::settings_dialog_hints());
        }
        Overlay::MetadataMenu => {
            let regions = menus::metadata_menu_interactions(context.theme, frame_area);
            register_menu(context, regions, DialogId::MetadataMenu);
        }
        Overlay::ConfirmDelete(ctx, _) => {
            let inner = dialogs::confirm_delete_inner(context.theme, frame_area, ctx);
            register_confirm(context, inner, ConfirmId::Delete);
        }
        Overlay::EditMetadata(state) => {
            let layout =
                dialogs::metadata_dialog_layout(context.theme, frame_area, state.filtered.len());
            register_dialog_list(
                context,
                layout.list,
                layout.list,
                state.offset(),
                state.filtered.len(),
                DialogId::Metadata,
            );
            register_hint_regions(
                context,
                layout.hints,
                dialogs::metadata_dialog_hints(state.focus, state.input.as_str().trim().is_empty()),
            );
            context.view.interactions.push(
                layout.input,
                InteractionKind::DialogInput(DialogInputId::Metadata),
            );
        }
        Overlay::EditFeelings(state) => {
            let layout = dialogs::feelings_dialog_layout(
                context.theme,
                frame_area,
                state.item_count(),
                &state.selected,
            );
            register_dialog_list(
                context,
                layout.list,
                layout.list,
                state.offset(),
                state.item_count(),
                DialogId::Feelings,
            );
            register_hint_regions(
                context,
                layout.hints,
                dialogs::feelings_dialog_hints(state.focus),
            );
            context.view.interactions.push(
                layout.input,
                InteractionKind::DialogInput(DialogInputId::Feelings),
            );
        }
        Overlay::EditMood(_) => {
            let layout = dialogs::mood_dialog_layout(context.theme, frame_area);
            context
                .view
                .interactions
                .push(layout.bar, InteractionKind::MoodBar(layout.bar));
            register_hint_regions(context, layout.hints, dialogs::mood_dialog_hints());
        }
        Overlay::EditLocation(state) => {
            let labels = state.list_labels();
            let layout = dialogs::location_dialog_layout(context.theme, frame_area, &labels);
            context.view.interactions.push(
                layout.list,
                InteractionKind::DialogList {
                    dialog: DialogId::Location,
                    viewport: layout.list.height,
                },
            );
            let mut y = layout.list.y;
            while y < layout.list.bottom() {
                let Some(index) =
                    dialogs::location_list_row_at(layout.list, &labels, state.offset(), y)
                else {
                    y += 1;
                    continue;
                };
                let start = y;
                y += 1;
                while y < layout.list.bottom()
                    && dialogs::location_list_row_at(layout.list, &labels, state.offset(), y)
                        == Some(index)
                {
                    y += 1;
                }
                context.view.interactions.push(
                    ratatui::layout::Rect::new(layout.list.x, start, layout.list.width, y - start),
                    InteractionKind::DialogRow {
                        dialog: DialogId::Location,
                        index,
                    },
                );
            }
            register_dialog_scrollbar(
                context,
                DialogId::Location,
                layout.list,
                labels.len(),
                state.offset(),
            );
            register_hint_regions(
                context,
                layout.hints,
                dialogs::location_dialog_hints(state.focus, state.query_looked_up),
            );
            context.view.interactions.push(
                layout.query,
                InteractionKind::DialogInput(DialogInputId::LocationQuery),
            );
            context.view.interactions.push(
                layout.name,
                InteractionKind::DialogInput(DialogInputId::LocationName),
            );
        }
        Overlay::ThemePicker(state) => {
            let hint_state =
                state.hint_state(app.appearance.chrome_override, app.appearance.color_mode);
            let layout = dialogs::theme_picker_layout(
                context.theme,
                frame_area,
                state.entries.len(),
                hint_state,
            );
            register_dialog_list(
                context,
                layout.list,
                layout.list,
                state.offset(),
                state.entries.len(),
                DialogId::ThemePicker,
            );
            register_hint_regions(
                context,
                layout.hints,
                &dialogs::theme_picker_hints(hint_state),
            );
        }
        Overlay::Filter(state) => {
            let layout = dialogs::filter_dialog_layout(context.theme, frame_area, state);
            for (tab, rect) in
                tab_strip::tab_rects::<crate::tui::features::filter::FilterTab>(layout.tabs)
            {
                context
                    .view
                    .interactions
                    .push(rect, InteractionKind::FilterTab(tab));
            }
            register_dialog_list(
                context,
                layout.list,
                layout.list,
                state.offset(),
                state.current_rows().len(),
                DialogId::Filter,
            );
            register_hint_regions(context, layout.hints, dialogs::filter_dialog_hints());
        }
        Overlay::Help { tab, scroll } => {
            let layout = menus::help_dialog_layout(context.theme, frame_area, *tab);
            if let Some(tabs) = layout.tabs {
                for (tab, rect) in tab_strip::tab_rects::<crate::tui::state::HelpTab>(tabs) {
                    context
                        .view
                        .interactions
                        .push(rect, InteractionKind::HelpTab(tab));
                }
            }
            register_dialog_scrollbar(
                context,
                DialogId::Help,
                layout.track,
                layout.total as usize,
                *scroll as usize,
            );
            register_hint_regions(context, layout.hints, menus::help_dialog_hints());
        }
        _ => {}
    }

    if let Some(editor) = app.editor.as_ref() {
        match editor.prompt {
            EditorPrompt::MetadataMenu => {
                let regions = menus::metadata_menu_interactions(context.theme, frame_area);
                register_menu(context, regions, DialogId::EditorMetadataMenu);
            }
            EditorPrompt::ConfirmDiscard { .. } => {
                let area = frames::editor_discard_confirm_area(context.theme, frame_area);
                register_confirm(
                    context,
                    frames::dialog_inner(context.theme, area),
                    ConfirmId::EditorDiscard,
                );
            }
            EditorPrompt::Help { scroll } => {
                let layout = menus::editor_shortcuts_layout(context.theme, frame_area);
                register_dialog_scrollbar(
                    context,
                    DialogId::EditorHelp,
                    layout.track,
                    layout.total as usize,
                    scroll as usize,
                );
                register_hint_regions(context, layout.hints, menus::editor_shortcuts_hints());
            }
            EditorPrompt::None => {}
        }
    }

    let field = match &app.overlay {
        crate::tui::state::Overlay::NewJournal(_) => Some((
            dialogs::new_journal_field_rect(context.theme, frame_area),
            TextFieldId::NewJournal,
        )),
        crate::tui::state::Overlay::EditMetadata(state) => {
            let layout =
                dialogs::metadata_dialog_layout(context.theme, frame_area, state.filtered.len());
            Some((
                dialogs::input_field_rect(layout.input, "Search / new: "),
                TextFieldId::Metadata,
            ))
        }
        crate::tui::state::Overlay::EditFeelings(state) => {
            let layout = dialogs::feelings_dialog_layout(
                context.theme,
                frame_area,
                state.item_count(),
                &state.selected,
            );
            Some((
                dialogs::input_field_rect(layout.input, "Search: "),
                TextFieldId::Feelings,
            ))
        }
        crate::tui::state::Overlay::EditLocation(state) => {
            let layout =
                dialogs::location_dialog_layout(context.theme, frame_area, &state.list_labels());
            context.view.interactions.push(
                dialogs::input_field_rect(layout.query, "Place / address / coords: "),
                InteractionKind::TextField(TextFieldId::LocationQuery),
            );
            Some((
                dialogs::input_field_rect(layout.name, "Name: "),
                TextFieldId::LocationName,
            ))
        }
        crate::tui::state::Overlay::None if app.nav.mode == crate::tui::app::Mode::Search => {
            entries_area
                .and_then(|area| entries::search_field_rect(context.theme, area))
                .map(|area| (area, TextFieldId::Search))
        }
        _ => None,
    };
    if let Some((area, id)) = field {
        context
            .view
            .interactions
            .push(area, InteractionKind::TextField(id));
    }
    // After the field and after the entry rows: the popup hangs over them, and
    // the hit test takes the last region pushed at a point. The whole frame is
    // registered, not just the rows, so a click on its edge stays with the popup.
    let suggestion_rows = app.search.suggestions.len();
    if app.suggestions_visible()
        && let Some(area) = entries_area
        && let Some(outer) = entries::search_suggestions_rect(context.theme, area, suggestion_rows)
        && let Some(list) =
            entries::search_suggestions_list_rect(context.theme, area, suggestion_rows)
    {
        register_dialog_list(
            context,
            outer,
            list,
            app.search.suggestions.offset(),
            suggestion_rows,
            DialogId::SearchSuggestions,
        );
    }
}

fn interaction_overlay_open(app: &AppModel) -> bool {
    app.has_overlay()
        || app
            .editor
            .as_ref()
            .is_some_and(|editor| !matches!(editor.prompt, EditorPrompt::None))
}

fn register_menu(
    context: &mut RenderContext<'_>,
    regions: menus::MenuInteractions,
    dialog: DialogId,
) {
    register_hint_regions(context, regions.footer, menus::metadata_menu_hints());
    for (area, index) in regions.rows {
        context
            .view
            .interactions
            .push(area, InteractionKind::DialogRow { dialog, index });
    }
}

/// Register a dialog list's clickable regions. `surface` is what swallows a
/// click — for a framed popup that is the whole frame, so its edge and padding
/// don't hand the click to whatever it hangs over; `list` is the drawn row band
/// that rows, viewport and scrollbar all come from. A dialog whose list fills
/// its frame passes the same rect for both.
fn register_dialog_list(
    context: &mut RenderContext<'_>,
    surface: ratatui::layout::Rect,
    list: ratatui::layout::Rect,
    offset: usize,
    len: usize,
    dialog: DialogId,
) {
    context.view.interactions.push(
        surface,
        InteractionKind::DialogList {
            dialog,
            viewport: list.height,
        },
    );
    for visible in 0..list.height as usize {
        let index = offset.saturating_add(visible);
        if index >= len {
            break;
        }
        context.view.interactions.push(
            ratatui::layout::Rect::new(list.x, list.y + visible as u16, list.width, 1),
            InteractionKind::DialogRow { dialog, index },
        );
    }
    register_dialog_scrollbar(context, dialog, list, len, offset);
}

fn register_hint_regions(
    context: &mut RenderContext<'_>,
    area: ratatui::layout::Rect,
    hints: &[Hint],
) {
    // Laid out at the very origin and width `render_hint_line` draws at: the gaps
    // absorb leftover width, so an inset would shift every chip.
    for (row, start, width, id) in footer::hint_regions(hints, area.width) {
        if row >= area.height {
            break;
        }
        let x = area.x + start;
        let width = width.min(area.right().saturating_sub(x));
        if width == 0 {
            continue;
        }
        context.view.interactions.push(
            ratatui::layout::Rect::new(x, area.y + row, width, 1),
            InteractionKind::Hint(id),
        );
    }
}

fn register_confirm(
    context: &mut RenderContext<'_>,
    inner: ratatui::layout::Rect,
    confirm: ConfirmId,
) {
    let (yes, no) = frames::confirm_button_rects(inner);
    context.view.interactions.push(
        yes,
        InteractionKind::ConfirmButton {
            confirm,
            destructive: true,
        },
    );
    context.view.interactions.push(
        no,
        InteractionKind::ConfirmButton {
            confirm,
            destructive: false,
        },
    );
}

fn register_rows(
    context: &mut RenderContext<'_>,
    area: ratatui::layout::Rect,
    rows: &[RowMeta],
    offset: usize,
    panel: PanelId,
) {
    let viewport_end = offset.saturating_add(area.height as usize);
    let mut cursor = 0usize;
    for row in rows {
        let row_end = cursor.saturating_add(row.height as usize);
        if let Some(index) = row.item_index {
            let visible_start = cursor.max(offset);
            let visible_end = row_end.min(viewport_end);
            if visible_start < visible_end {
                context.view.interactions.push(
                    ratatui::layout::Rect::new(
                        area.x,
                        area.y + visible_start.saturating_sub(offset) as u16,
                        area.width,
                        visible_end.saturating_sub(visible_start) as u16,
                    ),
                    InteractionKind::Row { panel, index },
                );
            }
        }
        cursor = row_end;
        if cursor >= viewport_end {
            break;
        }
    }
}

fn draw_overlays(theme: &crate::tui::theme::Theme, frame: &mut Frame<'_>, app: &mut AppModel) {
    // Any overlay dims what's behind it first, so dialogs float on a darkened
    // backdrop instead of sitting flush on the content.
    let editor_prompt_open = app
        .editor
        .as_ref()
        .is_some_and(|editor| !matches!(editor.prompt, EditorPrompt::None));
    if !matches!(app.overlay, crate::tui::state::Overlay::None) || editor_prompt_open {
        let area = frame.area();
        chrome::scrim(&app.appearance.theme, frame.buffer_mut(), area);
    }

    let hover = app.hover;
    let hovered_dialog_row = match hover {
        crate::tui::state::HoverTarget::DialogRow(index) => Some(index),
        _ => None,
    };
    let hovered_button = match hover {
        crate::tui::state::HoverTarget::ConfirmButton(yes) => Some(yes),
        _ => None,
    };
    let hovered_menu_hint = match hover {
        crate::tui::state::HoverTarget::FooterHint(id) => Some(id),
        _ => None,
    };

    if let crate::tui::state::Overlay::ConfirmDelete(ctx, selected) = &app.overlay {
        draw_confirm_delete(theme, frame, ctx, *selected, hovered_button);
    }

    if matches!(app.overlay, crate::tui::state::Overlay::MetadataMenu) {
        draw_metadata_menu(theme, frame, hovered_dialog_row, hovered_menu_hint);
    }

    if let crate::tui::state::Overlay::Settings(state) = &mut app.overlay {
        draw_settings_dialog(theme, frame, state, &app.services.config, hover);
    }

    if let crate::tui::state::Overlay::Help { tab, scroll } = &mut app.overlay {
        menus::draw_help(theme, frame, *tab, hover, scroll);
    }

    let picker_chrome = app.appearance.chrome_override;
    let picker_mode = app.appearance.color_mode;
    if let Some(state) = app.theme_picker_state_mut() {
        draw_theme_picker(theme, picker_chrome, picker_mode, frame, state, hover);
    }

    if let Some(input) = app.new_journal_input_mut() {
        draw_new_journal_input(theme, frame, input, hover);
    }

    if let Some(state) = app.edit_metadata_state_mut() {
        draw_edit_metadata_dialog(theme, frame, state, hover);
    }

    if let Some(state) = app.edit_feeling_state_mut() {
        draw_edit_feelings_dialog(theme, frame, state, hover);
    }

    if let Some(state) = app.edit_mood_state() {
        draw_edit_mood_dialog(theme, frame, state, hover);
    }

    if let Some(state) = app.filter_state_mut() {
        draw_filter_dialog(theme, frame, state, hover);
    }

    if let Some(state) = app.edit_location_state_mut() {
        draw_edit_location_dialog(theme, frame, state, hover);
    }

    if let Some(state) = app.image_viewer_state() {
        draw_image_viewer(theme, frame, state, &app.image.runtime);
    }

    if let crate::tui::state::Overlay::FetchingEnvironment(started) = &app.overlay {
        draw_fetching_environment(theme, frame, *started);
    }

    if let Some(editor) = app.editor.as_mut() {
        match &mut editor.prompt {
            EditorPrompt::MetadataMenu => {
                draw_metadata_menu(theme, frame, hovered_dialog_row, hovered_menu_hint)
            }
            EditorPrompt::Help { scroll } => draw_editor_shortcuts(theme, frame, hover, scroll),
            EditorPrompt::ConfirmDiscard { discard_selected } => {
                draw_editor_discard_confirm(theme, frame, *discard_selected, hovered_button)
            }
            EditorPrompt::None => {}
        }
    }
}

#[cfg(test)]
mod tests;
