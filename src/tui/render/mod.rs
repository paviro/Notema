mod chrome;
mod dialogs;
mod editor;
mod entries;
mod footer;
mod frames;
mod image_viewer;
pub(crate) mod insights;
mod journals;
mod layout;
mod markdown;
mod menus;
mod metadata;
mod pending;
mod reader;
mod table;
mod toasts;
mod unlock;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    widgets::{ListState, Paragraph},
};

use super::app::{AppModel, single_panel_is_active};
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
    EntryListGeometry, EntryMetadataValues, PanelGeometry, entry_metadata_layout, point_in_rect,
};
use super::ui::{
    ConfirmId, DialogId, DialogInputId, InteractionKind, RenderContext, TextFieldId,
    interaction::PanelId,
};
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
    draw_new_journal_input, draw_theme_picker,
};
pub(crate) use dialogs::{
    feelings_dialog_layout, location_dialog_layout, metadata_dialog_layout, theme_picker_layout,
};
use editor::draw_entry_editor;
use entries::draw_entry_list;
pub(crate) use footer::{Hint, HintId, footer_height, footer_hint_id_at_point, footer_lines};
#[cfg(test)]
pub(crate) use footer::{
    footer_hint_id_at, footer_text, hint_grid_text, hint_height, hint_id_at_wrapped,
};
pub(crate) use frames::{draw_editor_discard_confirm, draw_modal_frame};
use image_viewer::draw_image_viewer;
use insights::draw_journal_insights;
pub(crate) use insights::insights_tab_at;
#[cfg(test)]
pub(crate) use journals::JOURNAL_BOX_HEIGHT;
use journals::draw_journals;
pub(crate) use journals::{journal_list_rect, journal_row_height};
pub(crate) use layout::{TuiLayout, tui_layout};
pub(crate) use menus::{MetadataMenuMode, draw_editor_shortcuts, draw_metadata_menu};
pub(crate) use pending::{
    AccessNotice, draw_disable_notice, draw_pending_notice, draw_pending_request,
};
use reader::draw_selected_reader;
#[cfg(test)]
use reader::metadata_scrolls_with_body;
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
        if let Some(editor) = app.editor.as_mut() {
            // Single-column (the one-col viewer breakpoint) gets a tighter margin
            // than an expanded fullscreen editor on a wide terminal.
            let (side, top) = if single_panel_is_active(area.width) {
                (3, 1)
            } else {
                (5, 3)
            };
            draw_entry_editor(theme, frame, chunks[0], editor, side, top);
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
        context.view.journal_offset = Some(draw_journals(theme, frame, area, app));
        let (_, rows, list) = app.journal_rows(area.content);
        register_rows(
            context,
            list,
            &rows,
            app.nav.journal_list.offset(),
            PanelId::Journals,
        );
    }
    if let Some(area) = layout.entries {
        context.view.entry_offset = Some(draw_entry_list(theme, frame, area, app));
        let rows = app.entry_rows(area.text_width);
        register_rows(
            context,
            area.panel.content,
            &rows.meta,
            app.nav.entry_list.offset(),
            PanelId::Entries,
        );
    }
    if let Some(area) = layout.insights {
        draw_journal_insights(theme, frame, area.area, app, &mut context.view.insights);
    } else if let Some(area) = layout.reader {
        if let Some(editor) = app.editor.as_mut() {
            draw_entry_editor(theme, frame, area.area, editor, 5, 3);
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
    draw_overlays(theme, frame, app);
    register_overlay_interactions(
        context,
        app,
        area,
        layout.entries.map(|entries| entries.panel.area),
    );
    draw_toasts(theme, frame, app);
}

/// Register a pane scrollbar's grab region — the one-cell bar plus one column
/// on each side, so it is easier to hit — carrying the metrics the mouse
/// handler needs to map presses and drags to scroll offsets. Skipped when the
/// content fits (no bar is drawn — `render_scrollbar_if_needed`'s guard).
fn register_scrollbar(
    context: &mut RenderContext<'_>,
    which: crate::tui::app::ScrollbarDrag,
    panel_area: ratatui::layout::Rect,
    content_length: usize,
    viewport: u16,
    scroll: usize,
) {
    let bar = scrollbar_bar_rect(context.theme, panel_area);
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
    {
        let (_, meta, list_area) = app.journal_rows(area.content);
        let total_height = crate::tui::entry_rows::total_row_height(&meta);
        let scroll = context
            .view
            .journal_offset
            .unwrap_or_else(|| app.nav.journal_list.offset());
        register_scrollbar(
            context,
            ScrollbarDrag::Journals,
            area.area,
            total_height,
            list_area.height,
            scroll,
        );
    }

    let reader = &context.view.reader;
    let visible_start = reader.scroll as usize;
    let visible_end = visible_start.saturating_add(reader.content_rect.height as usize);
    for hit in &reader.links {
        if hit.line < visible_start || hit.line >= visible_end {
            continue;
        }
        let heading_line = match &hit.target {
            crate::tui::app::ReaderLinkTarget::Uri(uri) => {
                uri.strip_prefix('#').and_then(|anchor| {
                    reader
                        .headings
                        .iter()
                        .find(|heading| heading.anchor == anchor)
                        .map(|heading| heading.line)
                })
            }
            crate::tui::app::ReaderLinkTarget::Image(_) => None,
        };
        context.view.interactions.push(
            ratatui::layout::Rect::new(
                reader.content_rect.x.saturating_add(hit.start as u16),
                reader
                    .content_rect
                    .y
                    .saturating_add((hit.line - visible_start) as u16),
                hit.end.saturating_sub(hit.start) as u16,
                1,
            ),
            InteractionKind::Link {
                target: hit.target.clone(),
                heading_line,
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
        Overlay::SettingsMenu => {
            let regions = menus::settings_menu_interactions(context.theme, frame_area);
            register_menu(context, regions, DialogId::Settings);
        }
        Overlay::MetadataMenu => {
            let regions = menus::metadata_menu_interactions(
                context.theme,
                frame_area,
                MetadataMenuMode::Viewer,
            );
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
            let hint_state = state.hint_state();
            let layout = dialogs::theme_picker_layout(
                context.theme,
                frame_area,
                state.entries.len(),
                hint_state,
            );
            register_dialog_list(
                context,
                layout.list,
                state.offset(),
                state.entries.len(),
                DialogId::ThemePicker,
            );
            register_hint_regions(
                context,
                layout.hints,
                &dialogs::theme_picker_hints(
                    hint_state,
                    app.appearance.chrome_override,
                    app.appearance.color_mode,
                ),
            );
        }
        _ => {}
    }

    if let Some(editor) = app.editor.as_ref() {
        match editor.prompt {
            EditorPrompt::MetadataMenu => {
                let regions = menus::metadata_menu_interactions(
                    context.theme,
                    frame_area,
                    MetadataMenuMode::Editor,
                );
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
            EditorPrompt::Help { .. } | EditorPrompt::None => {}
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
                .and_then(entries::search_field_rect)
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
    context
        .view
        .interactions
        .push(regions.footer, InteractionKind::DialogClose(dialog));
    for (area, index) in regions.rows {
        context
            .view
            .interactions
            .push(area, InteractionKind::DialogRow { dialog, index });
    }
}

fn register_dialog_list(
    context: &mut RenderContext<'_>,
    list: ratatui::layout::Rect,
    offset: usize,
    len: usize,
    dialog: DialogId,
) {
    context.view.interactions.push(
        list,
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
}

fn register_hint_regions(
    context: &mut RenderContext<'_>,
    area: ratatui::layout::Rect,
    hints: &[Hint],
) {
    for y in area.y..area.bottom() {
        let mut x = area.x;
        while x < area.right() {
            let Some(id) = footer::hint_id_at_wrapped(
                hints,
                area.x.saturating_add(1),
                area.y,
                area.width.saturating_sub(1),
                x,
                y,
            ) else {
                x += 1;
                continue;
            };
            let start = x;
            x += 1;
            while x < area.right()
                && footer::hint_id_at_wrapped(
                    hints,
                    area.x.saturating_add(1),
                    area.y,
                    area.width.saturating_sub(1),
                    x,
                    y,
                ) == Some(id)
            {
                x += 1;
            }
            context.view.interactions.push(
                ratatui::layout::Rect::new(start, y, x - start, 1),
                InteractionKind::Hint(id),
            );
        }
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

    if let crate::tui::state::Overlay::ConfirmDelete(ctx, selected) = &app.overlay {
        draw_confirm_delete(theme, frame, ctx, *selected, hovered_button);
    }

    if matches!(app.overlay, crate::tui::state::Overlay::MetadataMenu) {
        draw_metadata_menu(theme, frame, MetadataMenuMode::Viewer, hovered_dialog_row);
    }

    if matches!(app.overlay, crate::tui::state::Overlay::SettingsMenu) {
        menus::draw_settings_menu(theme, frame, hovered_dialog_row);
    }

    if let crate::tui::state::Overlay::Help { scroll } = &mut app.overlay {
        menus::draw_help(theme, frame, scroll);
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
                draw_metadata_menu(theme, frame, MetadataMenuMode::Editor, hovered_dialog_row)
            }
            EditorPrompt::Help { scroll } => draw_editor_shortcuts(theme, frame, scroll),
            EditorPrompt::ConfirmDiscard { discard_selected } => {
                draw_editor_discard_confirm(theme, frame, *discard_selected, hovered_button)
            }
            EditorPrompt::None => {}
        }
    }
}

#[cfg(test)]
mod tests;
