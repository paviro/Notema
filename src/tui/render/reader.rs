use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use notema_domain::{Entry, EntryEncryptionState};
use std::path::Path;
use unicode_width::UnicodeWidthStr;

use crate::tui::{
    app::{
        AppModel, Focus, ReaderHeading, ReaderHint, ReaderHits, ReaderLinkHit, ReaderLinkTarget,
        RenderedEntryBody,
    },
    env_strip::EnvironmentRef,
    image::sole_image_ref,
    render::{
        count_label, layout::EntryBodyFrame, panel_block, render_centered_notice,
        render_scrollbar_if_needed, viewer_scroll,
    },
    state::HoverTarget,
    surface::PanelGeometry,
    theme::Theme,
};

use super::hints::HintLabeller;
use super::markdown::render_text_chunk;
use super::metadata::{EntryMetadata, draw_metadata_section, metadata_section_lines};

pub(crate) fn draw_selected_reader(
    active_theme: &crate::tui::theme::Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    app: &mut AppModel,
    reader_view: &mut ReaderHits,
) {
    if let Some((title, content)) = app.selected_reader() {
        let metadata = app
            .resolved_selected_entry()
            .map(Entry::metadata_bundle)
            .unwrap_or_default();
        let entry_path = app.selected_entry_target().map(|target| target.path);
        // Attachment links open in the OS default app only for plaintext entries;
        // an encrypted entry's assets are `.age` on disk, so its links stay inert.
        let attachments_openable = app
            .resolved_selected_entry()
            .is_some_and(|entry| entry.encryption_state == EntryEncryptionState::Plain);

        // The environment tables live on the entry, not the metadata bundle, so
        // they're passed alongside it; the editor resolves its own the same way.
        let environment = app
            .resolved_selected_entry()
            .map(EnvironmentRef::for_entry)
            .unwrap_or_default();
        let entry_metadata = EntryMetadata::for_entry(active_theme, &metadata, environment);

        *reader_view = draw_markdown_panel(
            active_theme,
            frame,
            app,
            PanelEntry {
                title: &title,
                content: &content,
                word_count: app.selected_entry_word_count(),
                metadata: entry_metadata,
                attachments_openable,
            },
            PanelPlacement {
                area,
                requested_scroll: app.nav.scroll.reader,
                focused: app.nav.focus == Focus::Reader,
                entry_path: entry_path.as_deref(),
            },
        );
    } else {
        let block = panel_block(active_theme, "Entry", app.nav.focus == Focus::Reader, None);
        let content = PanelGeometry::new(active_theme, area).content;
        frame.render_widget(block, area);
        super::panel_focus_stripe(active_theme, frame, area, app.nav.focus == Focus::Reader);
        render_centered_notice(active_theme, frame, content, "No entry selected");
    }
}

/// The entry content rendered by the markdown panel.
struct PanelEntry<'a> {
    title: &'a str,
    content: &'a str,
    /// Precomputed on the entry, so the panel title never re-tokenizes the body.
    word_count: usize,
    metadata: EntryMetadata<'a>,
    /// Whether links into the entry's own asset folder should open in the OS
    /// default app — set only for plaintext entries.
    attachments_openable: bool,
}

struct PanelPlacement<'a> {
    area: Rect,
    requested_scroll: u16,
    focused: bool,
    entry_path: Option<&'a Path>,
}

/// Draw the entry body and metadata, returning the frame's reader geometry: the
/// applied scroll, the clickable link hits, the body rect they map through, the
/// total line count (for scrollbar drag mapping), and any link-hint labels
/// painted. Both reader call sites — fullscreen and split-pane — come through
/// here.
fn draw_markdown_panel(
    active_theme: &crate::tui::theme::Theme,
    frame: &mut Frame<'_>,
    app: &AppModel,
    entry: PanelEntry<'_>,
    placement: PanelPlacement<'_>,
) -> ReaderHits {
    let PanelEntry {
        title,
        content,
        word_count,
        metadata,
        attachments_openable,
    } = entry;
    let PanelPlacement {
        area,
        requested_scroll,
        focused,
        entry_path,
    } = placement;
    let block = panel_block(
        active_theme,
        title,
        focused,
        Some(count_label(word_count, "word", "words")),
    );
    let frame_layout = EntryBodyFrame::new(
        active_theme,
        area,
        metadata.values(),
        app.services.config.ui.layout.reader_body(),
    );
    let body_rect = frame_layout.body;

    let width = body_rect.width as usize;
    // Memoized on (entry path, width, data version): the markdown parse + syntax
    // highlight + render is the reader's dominant per-frame cost, so a frame that
    // only scrolled, blinked, or ticked images reuses the rendered lines.
    let show_link_urls = app.services.config.ui.layout.reader.show_link_urls;
    let hints = hint_labels_shown(app).then(|| app.reader_hints.pending());
    let body = app.cached_entry_body(entry_path, width, hints, || {
        build_reader_body(
            active_theme,
            content,
            width,
            entry_path,
            show_link_urls,
            attachments_openable,
            hints,
        )
    });
    let mut lines = body.lines.clone();
    let pad = frame_layout.top_pad(lines.len());
    if pad > 0 {
        let mut padded = Vec::with_capacity(pad as usize + lines.len());
        padded.extend(std::iter::repeat_with(|| Line::from("")).take(pad as usize));
        padded.append(&mut lines);
        lines = padded;
    }
    // Hit indices map to screen rows downstream, so shift them by the prepend.
    let links: Vec<ReaderLinkHit> = body
        .links
        .iter()
        .cloned()
        .map(|mut hit| {
            hit.line += pad as usize;
            hit
        })
        .collect();
    let headings: Vec<ReaderHeading> = body
        .headings
        .iter()
        .cloned()
        .map(|mut heading| {
            heading.line += pad as usize;
            heading
        })
        .collect();
    if let Some(flash) = app.reader_anchor_flash.as_ref()
        && flash.until > std::time::Instant::now()
        && let Some(line) = lines.get_mut(flash.line)
    {
        *line = line
            .clone()
            .patch_style(Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD));
    }
    // A hovered link/label reverses its own ink into a solid highlight — the
    // app's strong-highlight idiom (see the anchor flash), unmistakable under
    // the cursor on every theme and chrome, using each theme's link color with
    // no per-theme tuning.
    let hovered_link = Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD);
    if let HoverTarget::ReaderLink { line, start, end } = app.hover {
        // A wrapped link name is several hit segments sharing one group;
        // highlight every segment so the whole name inverts as one link
        // rather than only the row under the cursor.
        let group = links
            .iter()
            .find(|hit| hit.line == line && hit.start == start && hit.end == end)
            .map(|hit| hit.group);
        if let Some(group) = group {
            for hit in links.iter().filter(|hit| hit.group == group) {
                if let Some(line) = lines.get_mut(hit.line) {
                    patch_line_range(line, hit.start, hit.end, hovered_link);
                }
            }
        }
    }
    if frame_layout.metadata_scrolls() {
        let meta_lines = metadata_section_lines(active_theme, body_rect.width, &metadata);
        if !meta_lines.is_empty() {
            let height = body_rect.height as usize;
            if lines.len() + meta_lines.len() < height {
                // Fits: bottom-attach the metadata to the pane's bottom edge,
                // matching the pinned layout on taller panes.
                lines.resize(height - meta_lines.len(), Line::from(""));
            } else {
                // Overflows: one blank line sets the metadata off from the body as
                // it scrolls into view.
                lines.push(Line::from(""));
            }
            lines.extend(meta_lines);
        }
    }
    let line_count = lines.len();
    let scroll = viewer_scroll(requested_scroll, line_count, body_rect.height);
    let body_rect = frame_layout.centered(line_count);

    frame.render_widget(block, area);
    super::panel_focus_stripe(active_theme, frame, area, focused);
    frame.render_widget(
        Paragraph::new(lines)
            .style(active_theme.text())
            .scroll((scroll, 0)),
        body_rect,
    );

    let mut hits = ReaderHits {
        content_rect: body_rect,
        scroll,
        line_count,
        links,
        headings,
        hints: Vec::new(),
        openable: body.openable,
    };
    // The chips are already drawn — they are text in the body. All that is left
    // is telling the model which label opens what.
    hits.hints = body
        .hints
        .iter()
        .map(|(label, target)| ReaderHint {
            label: label.clone(),
            target: target.clone(),
            heading_line: hits.heading_line_for(target),
        })
        .collect();

    if let Some(layout) = frame_layout.metadata {
        draw_metadata_section(active_theme, frame, layout, &metadata, app.hover);
    }

    render_scrollbar_if_needed(
        active_theme,
        frame,
        area,
        line_count,
        body_rect.height,
        scroll as usize,
        focused,
    );

    hits
}

/// Whether this frame lays link-hint chips into the body. Every condition that
/// suspends the mode lands here: a frame that lays none reports no labels, which
/// is what takes the mode down.
fn hint_labels_shown(app: &AppModel) -> bool {
    app.reader_hints.is_active()
        && app.nav.focus == Focus::Reader
        && app.editor.is_none()
        && !app.has_overlay()
}

/// Build the entry-body lines, replacing each lone in-folder image with a
/// clickable `[Image N …]` label and recording `(body line index, image index)`
/// so clicks and the viewer agree on numbering. Without an entry path the body
/// is rendered as-is.
fn build_body_lines(
    theme: &Theme,
    content: &str,
    width: usize,
    entry_path: Option<&Path>,
    show_urls: bool,
    attachments_openable: bool,
    mut hints: Option<&mut HintLabeller>,
) -> RenderedEntryBody {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut links: Vec<ReaderLinkHit> = Vec::new();
    let mut headings: Vec<ReaderHeading> = Vec::new();
    // Offsets each chunk's link `group` into a document-unique range so grouping
    // hover highlights never merge links from different chunks.
    let mut group_base = 0usize;

    let Some(entry_path) = entry_path else {
        lines.push(Line::from(""));
        append_chunk(
            &mut lines,
            &mut links,
            &mut headings,
            &mut group_base,
            render_text_chunk(theme, content, width, show_urls, None, false, hints),
        );
        dedupe_heading_anchors(&mut headings);
        // `hints`/`openable` belong to the labeller, so `build_reader_body` fills
        // them in once for both walks.
        return RenderedEntryBody {
            lines,
            links,
            headings,
            ..RenderedEntryBody::default()
        };
    };

    // See BODY_LEADING_BLANK — the editor pads by the same amount.
    lines.push(Line::from(""));
    let mut buffer = String::new();
    let mut image_index = 0usize;
    // True while the last emitted row was an image label with nothing buffered
    // since. Lets a blank source line right after an image emit an explicit blank
    // row instead of being swallowed by the empty buffer, preserving the gap.
    let mut after_image = false;

    for line in content.split('\n') {
        let Some((alt, _asset)) = sole_image_ref(line, entry_path) else {
            if after_image && buffer.is_empty() && line.trim().is_empty() {
                lines.push(Line::from(""));
                continue;
            }
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(line);
            after_image = false;
            continue;
        };

        if !buffer.is_empty() {
            // The renderer trims trailing blank lines from each chunk, so a blank
            // source line before the image would otherwise vanish. Note whether
            // the source had that gap and re-emit it explicitly after the chunk.
            let had_gap = buffer.ends_with('\n');
            append_chunk(
                &mut lines,
                &mut links,
                &mut headings,
                &mut group_base,
                render_text_chunk(
                    theme,
                    &buffer,
                    width,
                    show_urls,
                    Some(entry_path),
                    attachments_openable,
                    hints.as_deref_mut(),
                ),
            );
            if had_gap {
                lines.push(Line::from(""));
            }
            buffer.clear();
        }
        after_image = true;

        let chip = hints
            .as_mut()
            .and_then(|labeller| labeller.take(ReaderLinkTarget::Image(image_index)));
        let (label, click_width) = image_label_line(theme, image_index, &alt, chip);
        links.push(ReaderLinkHit {
            line: lines.len(),
            start: 0,
            end: click_width,
            target: ReaderLinkTarget::Image(image_index),
            group: group_base,
        });
        group_base += 1;
        lines.push(label);
        image_index += 1;
    }

    if !buffer.is_empty() {
        append_chunk(
            &mut lines,
            &mut links,
            &mut headings,
            &mut group_base,
            render_text_chunk(
                theme,
                &buffer,
                width,
                show_urls,
                Some(entry_path),
                attachments_openable,
                hints,
            ),
        );
    }

    dedupe_heading_anchors(&mut headings);
    RenderedEntryBody {
        lines,
        links,
        headings,
        ..RenderedEntryBody::default()
    }
}

/// The reader body, with link-hint chips laid in when `hints` is set.
///
/// Two walks: a counting labeller tallies the targets, fixing the label width;
/// a sized one then lays the chips into the text so wrapping makes room. The
/// counting walk hands out nothing, so it doubles as the unhinted body, and its
/// tally is `openable` either way. Memoized per (entry, width, hints), so the
/// double build is paid once per entry rather than once per frame.
fn build_reader_body(
    theme: &Theme,
    content: &str,
    width: usize,
    entry_path: Option<&Path>,
    show_urls: bool,
    attachments_openable: bool,
    hints: Option<&str>,
) -> RenderedEntryBody {
    let mut counter = HintLabeller::counting();
    let mut plain = build_body_lines(
        theme,
        content,
        width,
        entry_path,
        show_urls,
        attachments_openable,
        Some(&mut counter),
    );
    let openable = counter.requested();
    plain.openable = openable;
    let Some(pending) = hints else {
        return plain;
    };
    let mut labeller = HintLabeller::new(openable, pending);
    let mut body = build_body_lines(
        theme,
        content,
        width,
        entry_path,
        show_urls,
        attachments_openable,
        Some(&mut labeller),
    );
    body.openable = openable;
    body.hints = labeller.into_assigned();
    body
}

/// Append a rendered chunk, shifting its link/heading line indices (chunk-local)
/// to their position in the assembled body.
fn append_chunk(
    lines: &mut Vec<Line<'static>>,
    links: &mut Vec<ReaderLinkHit>,
    headings: &mut Vec<ReaderHeading>,
    group_base: &mut usize,
    chunk: super::markdown::RenderedChunk,
) {
    let base = lines.len();
    let group_offset = *group_base;
    links.extend(chunk.links.into_iter().map(|mut hit| {
        hit.line += base;
        hit.group += group_offset;
        hit
    }));
    headings.extend(chunk.headings.into_iter().map(|mut heading| {
        heading.line += base;
        heading
    }));
    lines.extend(chunk.lines);
    *group_base += chunk.link_count;
}

/// Disambiguate repeated heading anchors across the whole document the way the
/// renderer cannot per chunk: the second `intro` becomes `intro-1`, the third
/// `intro-2`, matching in-page anchor links.
fn dedupe_heading_anchors(headings: &mut [ReaderHeading]) {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for heading in headings.iter_mut() {
        let count = counts.entry(heading.anchor.clone()).or_default();
        if *count > 0 {
            heading.anchor = format!("{}-{count}", heading.anchor);
        }
        *count += 1;
    }
}

/// Patch `style` onto the spans of `line` fully inside the `[start, end)` column
/// range — the seam that lifts a hovered link name. The range comes from the
/// name span's own column/width, so span boundaries align with it.
fn patch_line_range(line: &mut Line<'static>, start: usize, end: usize, style: Style) {
    let mut column = 0usize;
    for span in &mut line.spans {
        let span_end = column.saturating_add(span.width());
        if column >= start && span_end <= end {
            span.style = span.style.patch(style);
        }
        column = span_end;
    }
}

/// A clickable `[Image N: alt]` label, numbered 1-based. The keyboard reaches it
/// through link-hint mode like any other target, so the label advertises only
/// the click.
fn image_label_line(
    theme: &Theme,
    index: usize,
    alt: &str,
    chip: Option<super::hints::Chip>,
) -> (Line<'static>, usize) {
    let alt = alt.trim();
    let number = index + 1;
    let head = if alt.is_empty() {
        format!("Image {number}")
    } else {
        format!("Image {number}: {alt}")
    };
    let label = format!("[{head}]");
    // The clickable span is the label alone. Any hint chip trails it as separate
    // runs, exactly as it does for a markdown link — where the chip is emitted
    // after the link is popped and so carries no link id. Measuring the whole
    // line here instead would drag the chip into the hit, and hovering the image
    // would light the chip up too.
    let width = UnicodeWidthStr::width(label.as_str());
    let mut spans = vec![Span::styled(label, theme.md_link())];
    if let Some(chip) = chip {
        spans.extend(
            chip.runs(theme)
                .into_iter()
                .map(|(text, style)| Span::styled(text, style)),
        );
    }
    (Line::from(spans), width)
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn entry_path_with_asset() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let assets = dir.path().join("2026-07-05T14-30-00-abc123.assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("x9k2.png"), b"img").unwrap();
        fs::write(assets.join("aa11.png"), b"img").unwrap();
        let entry_path = dir.path().join("2026-07-05T14-30-00-abc123.md");
        fs::write(&entry_path, b"entry").unwrap();
        (dir, entry_path)
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn target_uri(hit: &ReaderLinkHit) -> &str {
        match &hit.target {
            ReaderLinkTarget::Uri(uri) => uri,
            ReaderLinkTarget::Image(index) => panic!("expected a uri target, got image {index}"),
        }
    }

    /// Numbering is 1-based and unbounded — the eleventh image reads the same as
    /// the first, since no label advertises a key of its own any more.
    #[test]
    fn image_label_is_its_number_and_alt_and_nothing_else() {
        let label = |index, alt| {
            line_text(&image_label_line(&Theme::terminal_default(), index, alt, None).0)
        };
        assert_eq!(label(0, "sunset"), "[Image 1: sunset]");
        assert_eq!(label(3, ""), "[Image 4]");
        assert_eq!(label(10, "late"), "[Image 11: late]");
    }

    /// The clickable span stops at the label, so hovering an image lights the
    /// label alone — the same as a markdown link, whose chip is emitted after the
    /// link is popped and so never joins the hit.
    #[test]
    fn an_image_hint_chip_is_outside_the_clickable_span() {
        let mut labeller = HintLabeller::new(1, "");
        let chip = labeller.take(ReaderLinkTarget::Image(0));
        let (line, click_width) = image_label_line(&Theme::terminal_default(), 0, "sunset", chip);

        assert_eq!(click_width, "[Image 1: sunset]".len());
        assert!(
            line.width() > click_width,
            "the chip widens the line past the clickable span"
        );
    }

    /// Each lone in-folder image becomes a numbered clickable label, and its body
    /// line index is recorded so clicks map back to the right image.
    #[test]
    fn replaces_images_with_numbered_labels_and_records_positions() {
        let (_guard, entry_path) = entry_path_with_asset();
        let content = concat!(
            "Text above\n",
            "\n",
            "![a shot](2026-07-05T14-30-00-abc123.assets/x9k2.png)\n",
            "\n",
            "![](2026-07-05T14-30-00-abc123.assets/aa11.png)\n",
            "\n",
            "Text below",
        );

        let body = build_body_lines(
            &Theme::terminal_default(),
            content,
            40,
            Some(&entry_path),
            true,
            false,
            None,
        );

        let rendered: Vec<String> = body.lines.iter().map(line_text).collect();
        assert_eq!(
            rendered,
            vec![
                String::new(),
                "Text above".to_string(),
                String::new(),
                "[Image 1: a shot]".to_string(),
                String::new(),
                "[Image 2]".to_string(),
                String::new(),
                "Text below".to_string(),
            ],
        );
        // Each label records a link hit covering exactly its own text, in its
        // own group so hovering one never highlights the other.
        assert_eq!(
            body.links
                .iter()
                .map(|hit| (hit.line, hit.start, hit.end, hit.target.clone()))
                .collect::<Vec<_>>(),
            vec![
                (3, 0, rendered[3].len(), ReaderLinkTarget::Image(0)),
                (5, 0, rendered[5].len(), ReaderLinkTarget::Image(1)),
            ],
        );
        assert_ne!(body.links[0].group, body.links[1].group);
    }

    /// A lone `==` in prose must not turn the highlight on for the rest of the
    /// document; the state resets at every block boundary.
    #[test]
    fn unpaired_highlight_marker_does_not_leak_past_its_block() {
        use ratatui::style::Modifier;
        let body = build_body_lines(
            &Theme::terminal_default(),
            "a == b\n\nplain paragraph",
            40,
            None,
            true,
            false,
            None,
        );
        let plain = body
            .lines
            .iter()
            .find(|line| line_text(line).contains("plain"))
            .expect("the later paragraph renders");
        assert!(
            plain
                .spans
                .iter()
                .all(|span| !span.style.add_modifier.contains(Modifier::REVERSED)),
            "highlight leaked into a later block"
        );
    }

    /// Without an entry path (no selected entry) the body renders untouched.
    #[test]
    fn no_labels_without_entry_path() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "just text",
            40,
            None,
            true,
            false,
            None,
        );
        assert!(body.links.is_empty());
    }

    #[test]
    fn renderer_records_heading_anchors_and_link_cells() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "# My Heading\n\n[Jump](#my-heading)",
            40,
            None,
            true,
            false,
            None,
        );

        assert_eq!(
            body.headings,
            [ReaderHeading {
                anchor: "my-heading".to_string(),
                line: 1,
            }]
        );
        assert_eq!(body.links.len(), 1);
        assert_eq!(
            body.links[0].target,
            ReaderLinkTarget::Uri("#my-heading".into())
        );
        // The clickable region is the name; the target trails it in the faint
        // secondary style.
        let link_line = &body.lines[body.links[0].line];
        assert_eq!(link_line.spans[0].content, "Jump");
        assert_eq!(
            link_line.spans[0].style,
            Theme::terminal_default().md_link()
        );
        assert_eq!(link_line.spans.last().unwrap().content, " (#my-heading)");
        assert_eq!(
            link_line.spans.last().unwrap().style,
            Theme::terminal_default().muted()
        );
        assert_eq!((body.links[0].start, body.links[0].end), (0, 4));
    }

    /// A bare autolink renders once (no redundant parenthetical) and stays
    /// clickable over its own text.
    #[test]
    fn autolink_renders_once_and_stays_clickable() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "<https://example.com>",
            60,
            None,
            true,
            false,
            None,
        );

        assert_eq!(body.links.len(), 1);
        assert_eq!(
            body.links[0].target,
            ReaderLinkTarget::Uri("https://example.com".into())
        );
        let link_line = &body.lines[body.links[0].line];
        assert_eq!(line_text(link_line), "https://example.com");
    }

    /// With link URLs hidden, the faint `(url)` trailer is stripped from the
    /// display but the name stays clickable over the same columns.
    #[test]
    fn hidden_link_urls_strip_the_trailer_but_keep_the_link() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "See [the docs](https://example.com) now.",
            60,
            None,
            false,
            false,
            None,
        );

        let link_line = &body.lines[body.links[0].line];
        assert_eq!(line_text(link_line), "See the docs now.");
        assert_eq!(body.links.len(), 1);
        assert_eq!(
            body.links[0].target,
            ReaderLinkTarget::Uri("https://example.com".into())
        );
        // "See " is 4 cells, "the docs" is 8 — the hit still covers the name.
        assert_eq!((body.links[0].start, body.links[0].end), (4, 12));

        // Shown, the same source keeps the faint trailer.
        let shown = build_body_lines(
            &Theme::terminal_default(),
            "See [the docs](https://example.com) now.",
            60,
            None,
            true,
            false,
            None,
        );
        assert!(line_text(&shown.lines[shown.links[0].line]).contains("(https://example.com)"));
    }

    /// Six consecutive links whose names and ` (url)` trailers straddle wrap
    /// boundaries are all detected — the regression that lost every other link
    /// when semantics were re-scanned per display line.
    #[test]
    fn every_wrapped_consecutive_link_is_detected() {
        let source = "including [Setext](http://docutils.sourceforge.net/mirror/setext.html), \
[atx](http://www.aaronsw.com/2002/atx/), [Textile](http://textism.com/tools/textile/), \
[reStructuredText](http://docutils.sourceforge.net/rst.html), \
[Grutatext](http://www.triptico.com/software/grutatxt.html), \
and [EtText](http://ettext.taint.org/doc/) -- the end.";
        let expected = [
            "http://docutils.sourceforge.net/mirror/setext.html",
            "http://www.aaronsw.com/2002/atx/",
            "http://textism.com/tools/textile/",
            "http://docutils.sourceforge.net/rst.html",
            "http://www.triptico.com/software/grutatxt.html",
            "http://ettext.taint.org/doc/",
        ];

        let shown = build_body_lines(
            &Theme::terminal_default(),
            source,
            80,
            None,
            true,
            false,
            None,
        );
        let targets: Vec<&str> = shown.links.iter().map(target_uri).collect();
        assert_eq!(targets, expected);
        for hit in &shown.links {
            assert!(hit.end > hit.start);
            assert!(hit.line < shown.lines.len());
        }

        // Hidden URLs: still all six, wrap now computed against the shorter text,
        // and no URL leaks into any rendered line.
        let hidden = build_body_lines(
            &Theme::terminal_default(),
            source,
            80,
            None,
            false,
            false,
            None,
        );
        let hidden_targets: Vec<&str> = hidden.links.iter().map(target_uri).collect();
        assert_eq!(hidden_targets, expected);
        assert!(
            !hidden
                .lines
                .iter()
                .any(|line| line_text(line).contains("http://"))
        );
    }

    /// A link name that itself wraps records one clickable segment per display
    /// line it occupies, each covering a non-empty column span.
    #[test]
    fn a_wrapping_link_name_is_clickable_on_every_row() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "[the quick brown fox](https://example.com)",
            10,
            None,
            false,
            false,
            None,
        );

        assert!(body.links.len() >= 2);
        // Every segment belongs to the same link, so hovering any row highlights
        // the whole name rather than making it look like several links.
        let group = body.links[0].group;
        for hit in &body.links {
            assert_eq!(target_uri(hit), "https://example.com");
            assert_eq!(hit.group, group);
            assert!(hit.end > hit.start);
        }
    }

    /// Distinct links keep distinct groups, so hovering one never highlights the
    /// other even when they share a target.
    #[test]
    fn distinct_links_have_distinct_groups() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "[one](https://example.com) and [two](https://example.com)",
            80,
            None,
            true,
            false,
            None,
        );

        assert_eq!(body.links.len(), 2);
        assert_ne!(body.links[0].group, body.links[1].group);
    }

    /// A heading that wraps still slugs its whole title (not just the first
    /// display line) and anchors it to that first line.
    #[test]
    fn wrapping_heading_keeps_the_full_anchor_slug() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "## A Very Long Heading That Certainly Wraps Across Rows",
            20,
            None,
            true,
            false,
            None,
        );

        assert_eq!(
            body.headings,
            [ReaderHeading {
                anchor: "a-very-long-heading-that-certainly-wraps-across-rows".to_string(),
                line: 1,
            }]
        );
    }

    /// A relative link target stays styled but is not clickable.
    #[test]
    fn relative_links_are_styled_but_not_clickable() {
        let body = build_body_lines(
            &Theme::terminal_default(),
            "See [the pic](photo.png) here.",
            60,
            None,
            true,
            false,
            None,
        );

        assert!(body.links.is_empty());
        let styled = body.lines.iter().flat_map(|line| &line.spans).any(|span| {
            span.content == "the pic" && span.style == Theme::terminal_default().md_link()
        });
        assert!(styled);
    }

    /// An attachment link into the entry's own asset folder is a clickable hit
    /// for a plaintext entry (`attachments_openable`), but inert otherwise —
    /// encrypted entries record no hit, so the link gets no hover or click.
    #[test]
    fn attachment_link_is_clickable_only_when_openable() {
        let (_guard, entry_path) = entry_path_with_asset();
        let content = "Recording: [Audio attachment](2026-07-05T14-30-00-abc123.assets/x9k2.png)";

        let openable = build_body_lines(
            &Theme::terminal_default(),
            content,
            80,
            Some(&entry_path),
            true,
            true,
            None,
        );
        assert_eq!(openable.links.len(), 1);
        assert_eq!(
            target_uri(&openable.links[0]),
            "2026-07-05T14-30-00-abc123.assets/x9k2.png"
        );

        let inert = build_body_lines(
            &Theme::terminal_default(),
            content,
            80,
            Some(&entry_path),
            true,
            false,
            None,
        );
        assert!(inert.links.is_empty());
        // Even inert, the link name keeps its styling.
        let styled = inert.lines.iter().flat_map(|line| &line.spans).any(|span| {
            span.content == "Audio attachment" && span.style == Theme::terminal_default().md_link()
        });
        assert!(styled);
    }

    #[test]
    fn inline_image_is_not_opened_as_an_attachment() {
        let (_guard, entry_path) = entry_path_with_asset();
        let content = "Inline ![photo](2026-07-05T14-30-00-abc123.assets/x9k2.png)";

        let body = build_body_lines(
            &Theme::terminal_default(),
            content,
            80,
            Some(&entry_path),
            true,
            true,
            None,
        );

        assert!(body.links.is_empty());
    }
}

#[cfg(test)]
mod hit_span_tests {
    use super::*;

    /// The display-column slice of `line` covered by `[start, end)`.
    fn columns(line: &Line<'static>, start: usize, end: usize) -> String {
        let mut column = 0usize;
        let mut taken = String::new();
        for span in &line.spans {
            for ch in span.content.chars() {
                if column >= start && column < end {
                    taken.push(ch);
                }
                column += UnicodeWidthStr::width(ch.to_string().as_str());
            }
        }
        taken
    }

    /// Whatever produced a hit, its span must cover the target's own text and
    /// nothing else — a hint chip must never fall inside it, or hovering the
    /// target would light the chip up with it.
    ///
    /// Blind to which path built the hit: image labels are assembled by hand in
    /// `build_body_lines`, every other kind derived in `markdown.rs`, and only a
    /// check treating them alike catches one drifting from the other.
    #[test]
    fn no_hint_chip_falls_inside_a_clickable_span() {
        let dir = tempfile::tempdir().unwrap();
        let entry_dir = dir.path().join("work").join("2026-07-01");
        std::fs::create_dir_all(entry_dir.join("a.assets")).unwrap();
        std::fs::write(entry_dir.join("a.assets/pic.png"), []).unwrap();
        std::fs::write(entry_dir.join("a.assets/notes.pdf"), b"x").unwrap();
        let entry = entry_dir.join("a.md");

        let content = "See [one](https://example.com) and [notes](a.assets/notes.pdf).\n\n\
                       ![a shot](a.assets/pic.png)\n\n\
                       Jump to [details](#details).\n\n## Details\n\ntail";
        let body = build_reader_body(
            &Theme::terminal_default(),
            content,
            80,
            Some(&entry),
            false,
            true,
            Some(""),
        );

        assert_eq!(body.hints.len(), 4, "url, attachment, image and anchor");
        for hit in &body.links {
            let covered = columns(&body.lines[hit.line], hit.start, hit.end);
            assert!(
                !covered.contains('│') && !covered.contains("press"),
                "hit {:?} covers chip text: {covered:?}",
                hit.target
            );
        }
    }

    /// Every openable target gets a chip, including those that leave no hit: an
    /// empty link name, and the clickable-image idiom, whose alt text is tagged
    /// with the image rather than the link. Both still ask for a label, so a
    /// count taken from the hits leaves the last target without one.
    #[test]
    fn a_link_that_leaves_no_hit_still_consumes_a_label() {
        let content = "[](https://example.com)\n\n\
                       [![badge](https://example.org/b.svg)](https://example.org)\n\n\
                       And [a plain one](https://example.net) last.";
        let body = build_reader_body(
            &Theme::terminal_default(),
            content,
            80,
            None,
            false,
            false,
            Some(""),
        );

        let targets: Vec<&ReaderLinkTarget> =
            body.hints.iter().map(|(_, target)| target).collect();
        assert_eq!(
            targets,
            vec![
                &ReaderLinkTarget::Uri("https://example.com".to_string()),
                &ReaderLinkTarget::Uri("https://example.org/b.svg".to_string()),
                &ReaderLinkTarget::Uri("https://example.org".to_string()),
                &ReaderLinkTarget::Uri("https://example.net".to_string()),
            ]
        );
        let chips = body
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .filter(|span| span.content == "│ press ")
            .count();
        assert_eq!(
            chips,
            targets.len(),
            "every label was laid in, the last one included"
        );
    }
}
