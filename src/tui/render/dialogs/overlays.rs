use super::*;

pub(in crate::tui::render) fn draw_fetching_environment(
    theme: &Theme,
    frame: &mut Frame<'_>,
    started: Instant,
) {
    let dots = (started.elapsed().as_millis() / 400 % 3) as usize + 1;
    let message = format!(
        "Fetching weather and air quality{}{}",
        ".".repeat(dots),
        " ".repeat(3 - dots)
    );
    let width = surface_outer_width(theme, message.width() as u16);
    let area = centered_rect_fixed_size(width, 1 + dialog_frame_rows(theme), frame.area());
    let inner = draw_dialog_frame(theme, frame, area, "", false);
    frame.render_widget(Paragraph::new(message).alignment(Alignment::Center), inner);
}

/// The `(height, message)` a confirm-delete dialog needs for `ctx`. The message is
/// centered at the top; the Delete/Cancel buttons occupy the last inner row.
fn confirm_delete_content(theme: &Theme, ctx: &DeleteContext) -> (u16, String) {
    match ctx {
        DeleteContext::Entry { has_body: true } => (
            3 + dialog_frame_rows(theme),
            "Move entry to trash?".to_string(),
        ),
        DeleteContext::Entry { has_body: false } => (
            3 + dialog_frame_rows(theme),
            "Permanently delete entry?".to_string(),
        ),
        DeleteContext::Journal {
            name,
            trash_count,
            delete_count,
        } => {
            let line2 = match (*trash_count, *delete_count) {
                (0, d) => format!("{d} entries deleted permanently"),
                (t, 0) => format!("{t} entries moved to trash"),
                (t, d) => format!("{t} entries → trash, {d} deleted"),
            };
            let display = notema_storage::journal_display_name(name);
            (
                4 + dialog_frame_rows(theme),
                format!("Delete journal '{display}'?\n{line2}"),
            )
        }
    }
}

fn confirm_delete_area(theme: &Theme, frame_area: Rect, ctx: &DeleteContext) -> Rect {
    let (height, message) = confirm_delete_content(theme, ctx);
    let width = CONFIRM_DIALOG_WIDTH.max(
        message
            .lines()
            .map(|line| surface_outer_width(theme, line.len() as u16))
            .max()
            .unwrap_or(0),
    );
    super::centered_rect_fixed_size(width, height, frame_area)
}

/// The content rect of the confirm-delete dialog, so the mouse handler can
/// hit-test the buttons against the same geometry the draw uses.
pub(crate) fn confirm_delete_inner(theme: &Theme, frame_area: Rect, ctx: &DeleteContext) -> Rect {
    dialog_inner(theme, confirm_delete_area(theme, frame_area, ctx))
}

pub(in crate::tui::render) fn draw_confirm_delete(
    theme: &Theme,
    frame: &mut Frame<'_>,
    ctx: &DeleteContext,
    selected: bool,
    hovered: Option<bool>,
) {
    let (_, message) = confirm_delete_content(theme, ctx);
    let area = confirm_delete_area(theme, frame.area(), ctx);
    let inner = draw_dialog_frame(theme, frame, area, "Confirm Delete", true);

    // Message at the top, the Delete/Cancel buttons on the last inner row.
    for (i, line) in message.lines().enumerate() {
        let line_area = Rect {
            y: inner.y + i as u16,
            height: 1,
            ..inner
        };
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), line_area);
    }
    render_confirm_buttons(theme, frame, inner, "Delete", "Cancel", selected, hovered);
}

pub(in crate::tui::render) fn draw_new_journal_input(
    theme: &Theme,
    frame: &mut Frame<'_>,
    input: &mut TextInput,
    hover: HoverTarget,
) {
    let area = super::centered_rect_fixed_size(
        NEW_JOURNAL_DIALOG_WIDTH,
        3 + dialog_frame_rows(theme),
        frame.area(),
    );
    let inner = draw_dialog_frame(theme, frame, area, "New Journal", true);

    let label = "Name: ";
    frame.render_widget(Paragraph::new(label), inner);
    let field = new_journal_field_rect(theme, frame.area());
    let hovered = hovered_field(hover, field);
    input.render_in(theme, frame, field, true, hovered);

    let hint = Rect {
        y: inner.y + 2,
        height: 1,
        ..inner
    };
    frame.render_widget(Paragraph::new("Enter saves | Esc cancels"), hint);
}

pub(crate) fn new_journal_field_rect(theme: &Theme, frame_area: Rect) -> Rect {
    let area = super::centered_rect_fixed_size(
        NEW_JOURNAL_DIALOG_WIDTH,
        3 + dialog_frame_rows(theme),
        frame_area,
    );
    let inner = dialog_inner(theme, area);
    let label_width = "Name: ".len() as u16;
    Rect {
        x: inner.x + label_width,
        y: inner.y,
        width: inner.width.saturating_sub(label_width),
        height: 1,
    }
}

pub(in crate::tui::render) fn draw_edit_metadata_dialog(
    theme: &Theme,
    frame: &mut Frame<'_>,
    state: &mut EditMetadataState,
    hover: HoverTarget,
) {
    let layout = metadata_dialog_layout(theme, frame.area(), state.filtered.len());
    let title = state.kind.title();

    let list_focused = state.focus == EditMetadataFocus::List;
    let input_focused = state.focus == EditMetadataFocus::Input;

    state.normalize_list_state();
    let list_lines = state.filtered.len();
    let max_visible = layout.list.height;
    let max_offset = list_lines.saturating_sub(max_visible as usize);
    let scroll = state.offset().min(max_offset);
    state.list.set_offset(scroll);

    let items: Vec<ListItem<'_>> = if state.filtered.is_empty() {
        let text = if state.input.is_empty() {
            format!("(no {title} yet)").to_lowercase()
        } else {
            "(no matches)".to_string()
        };
        vec![ListItem::new(Line::from(text))]
    } else {
        let hovered_row = hovered_dialog_row(hover);
        // The hover lift defers only to a selection that's actually drawn —
        // with the input focused, the highlight is hidden and the selected
        // row must still respond to the mouse.
        let shown_selection = state.selected_index().filter(|_| list_focused);
        state
            .filtered
            .iter()
            .enumerate()
            .map(|(index, idx)| {
                let (tag, freq) = &state.all_values[*idx];
                let checked = state.selected.iter().any(|t| t.eq_ignore_ascii_case(tag));
                let marker = if checked { "[x]" } else { "[ ]" };
                let item = ListItem::new(dot_leader_line(
                    theme,
                    Span::raw(format!("{marker} {tag}")),
                    Span::styled(freq.to_string(), theme.muted()),
                    layout.list.width,
                    Some(index) == shown_selection,
                ));
                if Some(index) == hovered_row && Some(index) != shown_selection {
                    item.style(theme.hover())
                } else {
                    item
                }
            })
            .collect()
    };

    draw_dialog_frame(theme, frame, layout.area, &format!("Edit {title}"), true);
    render_lines_in_area(
        frame,
        [Line::from(Span::styled(
            format!(" {title} "),
            theme.heading(),
        ))],
        layout.inner,
    );
    render_separator(theme, frame, layout.list_top_separator);
    let list = List::new(items).highlight_style(theme.selection());
    let mut render_state = list_state_for_render(
        state.selected_index(),
        scroll,
        layout.list.height,
        list_focused && !state.filtered.is_empty(),
    );
    frame.render_stateful_widget(list, layout.list, &mut render_state);
    render_separator(theme, frame, layout.list_bottom_separator);
    render_search_field(
        theme,
        frame,
        layout.input,
        "Search / new: ",
        &mut state.input,
        input_focused,
        hover,
    );
    render_hint_line(
        theme,
        frame,
        metadata_dialog_hints(state.focus, state.input.as_str().trim().is_empty()),
        layout.hints,
        hover,
    );
    render_scrollbar_if_needed(
        theme,
        frame,
        layout.area,
        list_lines,
        max_visible,
        scroll,
        true,
    );
}

pub(in crate::tui::render) fn draw_edit_mood_dialog(
    theme: &Theme,
    frame: &mut Frame<'_>,
    state: &EditMoodState,
    hover: HoverTarget,
) {
    let layout = mood_dialog_layout(theme, frame.area());

    draw_dialog_frame(theme, frame, layout.area, "Edit Mood", true);

    let right_label = " Blissful";

    // Empty spacer row
    let spacer_y = layout.inner.y;
    if spacer_y < layout.inner.y + layout.inner.height {
        frame.render_widget(
            Paragraph::new(Line::from("")),
            Rect {
                x: layout.inner.x,
                y: spacer_y,
                width: layout.inner.width,
                height: 1,
            },
        );
    }

    // Mood bar row
    let right_w = right_label.len() as u16;
    let bar_y = layout.inner.y + 1;
    if bar_y < layout.inner.y + layout.inner.height {
        let bar_rect = Rect {
            x: layout.inner.x,
            y: bar_y,
            width: layout.inner.width,
            height: 1,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(10),
                Constraint::Min(3),
                Constraint::Length(right_w),
            ])
            .split(bar_rect);
        frame.render_widget(Paragraph::new("Miserable "), chunks[0]);
        frame.render_widget(MoodBar::new(theme, state.draft), chunks[1]);
        frame.render_widget(Paragraph::new(right_label), chunks[2]);
    }

    // Value number centred below the bar
    if layout.value.y < layout.inner.y + layout.inner.height {
        frame.render_widget(
            Paragraph::new(Line::from(format!("{}", state.draft))).alignment(Alignment::Center),
            layout.value,
        );
    }

    // Hint line
    if layout.hints.y < layout.inner.y + layout.inner.height {
        render_hint_line(theme, frame, mood_dialog_hints(), layout.hints, hover);
    }
}

pub(in crate::tui::render) fn draw_edit_location_dialog(
    theme: &Theme,
    frame: &mut Frame<'_>,
    state: &mut EditLocationState,
    hover: HoverTarget,
) {
    let showing_candidates = state.showing_candidates();
    let labels = state.list_labels();
    let item_count = labels.len();
    // Size the dialog to the wrapped row span so multi-line rows aren't clipped.
    let layout = location_dialog_layout(theme, frame.area(), &labels);

    state.normalize_list_state();
    let max_visible = layout.list.height;
    let max_offset = item_count.saturating_sub(max_visible as usize);
    let scroll = state.offset().min(max_offset);
    state.list.set_offset(scroll);

    let list_focused = state.focus == EditLocationFocus::List;
    let dim = theme.muted();
    let bold = theme.heading();

    draw_dialog_frame(theme, frame, layout.area, "Edit Location", true);

    render_lines_in_area(
        frame,
        [Line::from(Span::styled(" Location ", bold))],
        layout.title,
    );
    render_separator(theme, frame, layout.title_separator);

    let query_focused = state.focus == EditLocationFocus::Query;
    let name_focused = state.focus == EditLocationFocus::Name;
    render_search_field(
        theme,
        frame,
        layout.query,
        "Place / address / coords: ",
        &mut state.query,
        query_focused,
        hover,
    );
    render_search_field(
        theme,
        frame,
        layout.name,
        "Name: ",
        &mut state.name,
        name_focused,
        hover,
    );

    // Status line: reflects the in-flight/last lookup, or the resolved value.
    let status_line = match &state.status {
        LocationResolveStatus::Idle => {
            match state.resolved.as_ref().and_then(|l| l.display_label()) {
                Some(label) => Line::from(Span::styled(label, dim)),
                None => Line::from(Span::styled(
                    "Enter a place, address, or \"lat, lon\", then press enter",
                    dim,
                )),
            }
        }
        LocationResolveStatus::Resolving => Line::from(Span::styled("Resolving…", dim)),
        LocationResolveStatus::NoMatch => Line::from(Span::styled("No matches found", dim)),
        LocationResolveStatus::Error(error) => Line::from(Span::styled(error.clone(), dim)),
        LocationResolveStatus::Resolved => {
            match state.resolved.as_ref().and_then(|l| l.display_label()) {
                Some(label) => Line::from(vec![Span::styled("✓ ", bold), Span::raw(label)]),
                None => Line::from(Span::styled("Resolved", dim)),
            }
        }
    };
    render_lines_in_area(frame, [status_line], layout.status);

    render_separator(theme, frame, layout.list_separator);

    let heading = if showing_candidates {
        " Matches "
    } else {
        " Recent places "
    };
    render_lines_in_area(
        frame,
        [Line::from(Span::styled(heading, bold))],
        layout.heading,
    );

    // Wrap long rows onto continuation lines (aligned under the first) instead of
    // clipping them.
    let items: Vec<ListItem<'_>> = if labels.is_empty() {
        let text = if showing_candidates {
            "(no matches)"
        } else {
            "(no saved places yet)"
        };
        vec![ListItem::new(Line::from(text))]
    } else {
        let hovered_row = hovered_dialog_row(hover);
        // Defer only to a drawn selection: with a text field focused, the
        // highlight is hidden and the selected row must still hover.
        let shown_selection = state.selected_index().filter(|_| list_focused);
        labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let lines: Vec<Line<'static>> = location_row_lines(label, layout.list.width)
                    .into_iter()
                    .map(Line::from)
                    .collect();
                let item = ListItem::new(lines);
                if Some(index) == hovered_row && Some(index) != shown_selection {
                    item.style(theme.hover())
                } else {
                    item
                }
            })
            .collect()
    };

    let list = List::new(items).highlight_style(theme.selection());
    let mut render_state = list_state_for_render(
        state.selected_index(),
        scroll,
        layout.list.height,
        list_focused && item_count > 0,
    );
    frame.render_stateful_widget(list, layout.list, &mut render_state);

    render_hint_line(
        theme,
        frame,
        location_dialog_hints(state.focus, state.query_looked_up),
        layout.hints,
        hover,
    );
    render_scrollbar_if_needed(
        theme,
        frame,
        layout.area,
        item_count,
        max_visible,
        scroll,
        true,
    );
}

pub(in crate::tui::render) fn draw_edit_feelings_dialog(
    theme: &Theme,
    frame: &mut Frame<'_>,
    state: &mut EditFeelingState,
    hover: HoverTarget,
) {
    let rows = state.visible_rows();
    let layout = feelings_dialog_layout(theme, frame.area(), rows.len(), &state.selected);
    let filtering = state.is_filtering();
    let list_focused = state.focus == EditMetadataFocus::List;
    let input_focused = state.focus == EditMetadataFocus::Input;

    state.normalize_list_state();
    let list_lines = rows.len();
    let max_visible = layout.list.height;
    let max_offset = list_lines.saturating_sub(max_visible as usize);
    let scroll = state.offset().min(max_offset);
    state.list.set_offset(scroll);

    let hovered_row = hovered_dialog_row(hover);
    // Defer only to a drawn selection: with the search field focused, the
    // highlight is hidden and the selected row must still hover.
    let shown_selection = state.selected_index().filter(|_| list_focused);
    let items: Vec<ListItem<'_>> = if rows.is_empty() {
        vec![ListItem::new(Line::from("(no matches)"))]
    } else {
        rows.iter()
            .enumerate()
            .map(|(index, row)| {
                let selected_row = Some(index) == shown_selection;
                let item = match *row {
                    FeelingRow::Header { group } => {
                        let g = &state.groups[group];
                        let bold = theme.heading();
                        // ▾ open, ▸ collapsed. The disclosure trails the name so the
                        // selected-count can sit at the right edge on a dot leader.
                        let disclosure = if state.expanded[group] {
                            theme.glyphs().expanded
                        } else {
                            theme.glyphs().collapsed
                        };
                        let label = Span::styled(format!("{} {disclosure}", g.name), bold);
                        let selected = state.group_selected_count(group);
                        if selected > 0 {
                            ListItem::new(dot_leader_line(
                                theme,
                                label,
                                Span::styled(selected.to_string(), theme.muted()),
                                layout.list.width,
                                selected_row,
                            ))
                        } else {
                            ListItem::new(Line::from(label))
                        }
                    }
                    FeelingRow::Feeling { group, feeling } => {
                        let g = &state.groups[group];
                        let name = g.feelings[feeling].name;
                        let checked = state.selected.iter().any(|value| value == name);
                        let marker = if checked { "[x]" } else { "[ ]" };
                        if filtering {
                            // Headers are hidden while filtering, so pin each match's
                            // group to the right edge for context.
                            ListItem::new(dot_leader_line(
                                theme,
                                Span::raw(format!("{marker} {name}")),
                                Span::styled(g.name.to_string(), theme.muted()),
                                layout.list.width,
                                selected_row,
                            ))
                        } else {
                            ListItem::new(Line::from(format!("   {marker} {name}")))
                        }
                    }
                };
                if Some(index) == hovered_row && Some(index) != shown_selection {
                    item.style(theme.hover())
                } else {
                    item
                }
            })
            .collect()
    };

    draw_dialog_frame(theme, frame, layout.area, "Edit Feelings", true);
    render_lines_in_area(
        frame,
        [Line::from(Span::styled(" Feelings ", theme.heading()))],
        layout.inner,
    );
    render_separator(theme, frame, layout.list_top_separator);
    let list = List::new(items).highlight_style(theme.selection());
    let mut render_state = list_state_for_render(
        state.selected_index(),
        scroll,
        layout.list.height,
        list_focused && !rows.is_empty(),
    );
    frame.render_stateful_widget(list, layout.list, &mut render_state);
    render_separator(theme, frame, layout.list_bottom_separator);
    render_search_field(
        theme,
        frame,
        layout.input,
        "Search: ",
        &mut state.input,
        input_focused,
        hover,
    );

    // The "Selected:" label is bold and continuation lines align under it.
    let bold = theme.heading();
    let selected_rows = feelings_selected_rows(&state.selected, layout.selected.width);
    let summary: Vec<Line<'_>> = if selected_rows.is_empty() {
        vec![Line::from(vec![
            Span::styled("Selected:", bold),
            Span::raw(" none"),
        ])]
    } else {
        selected_rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let joined = row
                    .iter()
                    .map(|&i| state.selected[i].as_str())
                    .collect::<Vec<_>>()
                    .join(" | ");
                if index == 0 {
                    Line::from(vec![
                        Span::styled("Selected:", bold),
                        Span::raw(format!(" {joined}")),
                    ])
                } else {
                    Line::from(joined)
                }
            })
            .collect()
    };
    render_lines_in_area(frame, summary, layout.selected);
    render_hint_line(
        theme,
        frame,
        feelings_dialog_hints(state.focus),
        layout.hints,
        hover,
    );
    render_scrollbar_if_needed(
        theme,
        frame,
        layout.area,
        list_lines,
        max_visible,
        scroll,
        true,
    );
}
