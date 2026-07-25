use ratatui::{Frame, layout::Rect};

use crate::config::BodyLayout;
use crate::tui::{
    editor_state::{EditorPrompt, EntryEditor},
    render::{
        count_label,
        layout::{BODY_LEADING_BLANK, EntryBodyFrame},
        panel_block, render_scrollbar_if_needed,
    },
    state::HoverTarget,
    theme::Theme,
};

use super::metadata::{EntryMetadata, draw_metadata_section};

/// Draw the internal editor in the entry-view pane: the same bordered panel as
/// the viewer, with the `ratatui-textarea` buffer as the body and the buffered
/// metadata pinned below it. Frames the body with `body_layout` exactly as the
/// viewer does, so switching between the two doesn't move the text.
pub(crate) fn draw_entry_editor(
    active_theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &mut EntryEditor,
    body_layout: BodyLayout,
) {
    // Before the textarea renders (it reads the spans during render) and before
    // the title's word count, and ahead of the immutable metadata borrow below.
    // No-op unless the body changed since the last frame.
    editor.refresh_for_body(active_theme);

    let block = panel_block(
        active_theme,
        editor.title(),
        true,
        Some(count_label(editor.word_count(), "word", "words")),
    );
    frame.render_widget(block, area);
    super::panel_focus_stripe(active_theme, frame, area, true);

    // The viewer's builder and environment, over the buffered metadata: the
    // block has to measure the same in both, or the two disagree about when the
    // pane is too short to pin it — and to center.
    let metadata =
        EntryMetadata::for_entry(active_theme, &editor.metadata, editor.environment_ref());

    // The metadata section pins below the body only while the pane can still give the
    // body its minimum height; once the metadata would push it under that, it's
    // dropped and the whole pane goes to the textarea. (The viewer instead folds
    // metadata into its scroll there, but the editor's scroll is cursor-driven and
    // can't reach a read-only block past the text.) Nothing is lost: the Ctrl+G
    // dialogs show the current values as you edit them, and the viewer shows them in
    // full on save.
    let frame_layout = EntryBodyFrame::new(active_theme, area, metadata.values(), body_layout);
    let text_rect = frame_layout.body;
    // Wrap for this width up front: the count below decides the rect, and the
    // textarea would otherwise still hold the previous render's map.
    editor.textarea.layout_for(text_rect);
    let line_count = editor.textarea.text_screen_line_count() + BODY_LEADING_BLANK as usize;
    // Into the textarea's scroll rather than the rect, so the padding scrolls
    // away with the text instead of costing viewport height.
    editor
        .textarea
        .set_top_padding(BODY_LEADING_BLANK + frame_layout.top_pad(line_count));
    let text_rect = frame_layout.centered(line_count);

    // While selecting, draw the reversed-block caret so the boundary character
    // reads as part of the selection (a thin bar can't fill that cell); otherwise
    // the theme's cursor style — by default unstyled, leaving the native bar
    // cursor placed below as the only caret.
    let selecting = editor.textarea.selection_range().is_some();
    editor
        .textarea
        .set_cursor_line_style(active_theme.cursor_line());
    editor
        .textarea
        .set_selection_style(active_theme.selection());
    editor
        .textarea
        .set_placeholder_style(active_theme.placeholder());
    editor.textarea.set_cursor_style(if selecting {
        active_theme.selection()
    } else {
        active_theme.cursor()
    });

    editor.text_rect = text_rect;
    frame.render_widget(&editor.textarea, text_rect);

    // Native terminal bar cursor, only while typing without a selection and with
    // no modal prompt over the editor. screen_cursor().row is the absolute wrapped
    // row, which excludes the top padding the scroll top counts; adding it back
    // gives the viewport-relative row. Wrap mode has no horizontal scroll, so col
    // maps directly. Valid only after render.
    if !selecting && matches!(editor.prompt, EditorPrompt::None) {
        let sc = editor.textarea.screen_cursor();
        let scroll = editor.textarea.scroll_offset() as usize;
        let row = sc.row + editor.textarea.top_padding() as usize;
        if let Some(rel) = row.checked_sub(scroll) {
            let x = text_rect.x + sc.col as u16;
            let y = text_rect.y + rel as u16;
            if x < text_rect.x + text_rect.width && y < text_rect.y + text_rect.height {
                frame.set_cursor_position((x, y));
            }
        }
    }

    // Scroll offset and wrapped-line count are only valid after the textarea has
    // rendered (it stores them during render), so read them here.
    render_scrollbar_if_needed(
        active_theme,
        frame,
        area,
        editor.textarea.screen_line_count(),
        text_rect.height,
        editor.textarea.scroll_offset() as usize,
        // The editor is always the active surface while shown.
        true,
    );

    if let Some(layout) = frame_layout.metadata {
        // The editor's own metadata preview has no clickable chips, so no hover.
        draw_metadata_section(active_theme, frame, layout, &metadata, HoverTarget::None);
    }
}
