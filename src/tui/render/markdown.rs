use pulldown_cmark::{
    Alignment as MarkdownAlignment, BlockQuoteKind, CodeBlockKind, Event as MarkdownEvent,
    HeadingLevel, Options as MarkdownOptions, Parser as MarkdownParser, Tag as MarkdownTag,
    TagEnd as MarkdownTagEnd,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

use crate::tui::app::{ReaderHeading, ReaderLinkHit};
use crate::tui::theme::Theme;

mod table;
mod wrapping;
use wrapping::{rich_from_line, wrap_line, wrap_rich};

/// The rendered form of one markdown chunk: the wrapped display lines plus the
/// clickable link regions and heading anchors discovered structurally during
/// rendering. Link/heading `line` indices are relative to `lines`.
pub(super) struct RenderedChunk {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) links: Vec<ReaderLinkHit>,
    pub(super) headings: Vec<ReaderHeading>,
    /// Number of link ids minted for this chunk, so the caller can offset the
    /// per-chunk `ReaderLinkHit::group` into a document-unique range.
    pub(super) link_count: usize,
}

/// Render a chunk of markdown text into owned (`'static`) lines. `show_urls`
/// controls whether a link's ` (url)` trailer is emitted; it is applied here,
/// before wrapping, so hidden URLs never skew wrap boundaries.
///
/// When `attachments_openable` is set, links pointing inside `entry_path`'s own
/// asset folder (imported audio/video/pdf attachments) become clickable hits so
/// they can be opened in the OS default app. It is left unset for encrypted
/// entries, whose on-disk assets are `.age` and cannot be handed to the OS.
pub(super) fn render_text_chunk(
    theme: &Theme,
    text: &str,
    width: usize,
    show_urls: bool,
    entry_path: Option<&Path>,
    attachments_openable: bool,
) -> RenderedChunk {
    MarkdownTerminalRenderer::new(theme, width, show_urls, entry_path, attachments_openable)
        .render(text)
}

/// A styled run tagged with the link it belongs to, if any. The renderer
/// accumulates the current logical line as these so link identity survives
/// wrapping; the `link` tag is an index into [`MarkdownTerminalRenderer::link_targets`].
#[derive(Clone)]
struct RichSpan {
    content: String,
    style: Style,
    link: Option<usize>,
}

struct MarkdownTerminalRenderer<'a> {
    theme: &'a Theme,
    width: usize,
    show_urls: bool,
    lines: Vec<Line<'static>>,
    current: Vec<RichSpan>,
    styles: Vec<Style>,
    /// The open links/images as `(target, visible text, id)`. The text
    /// accumulates as the link's inner events stream, so the closing tag can drop
    /// the parenthetical when the visible name already is the target (autolinks).
    /// `id` indexes [`Self::link_targets`] and tags the name's characters.
    links: Vec<(String, String, usize)>,
    /// Every link target seen, indexed by the id stored on `RichSpan`s and used
    /// to resolve a recorded hit's URL.
    link_targets: Vec<LinkTarget>,
    /// Clickable link regions discovered while wrapping, in display-line coords.
    link_hits: Vec<ReaderLinkHit>,
    /// Heading anchors, built from the complete (pre-wrap) heading text.
    headings: Vec<ReaderHeading>,
    /// The visible text of the open heading, accumulated so its anchor slug is
    /// built from the whole title even when the heading wraps across lines.
    heading_text: Option<String>,
    /// Open block containers (blockquotes and lists) in nesting order, so a
    /// blockquote inside a list follows the list's indent and vice versa.
    containers: Vec<Container>,
    code: Option<MarkdownCodeBlock>,
    table: Option<MarkdownTable>,
    separate_next_block: bool,
    highlight_open: bool,
    /// The entry being rendered, used to recognize links into its own asset
    /// folder as openable attachments.
    entry_path: Option<PathBuf>,
    /// Whether asset-folder attachment links should be recorded as clickable
    /// hits — set only for plaintext (unencrypted) entries.
    attachments_openable: bool,
}

struct LinkTarget {
    target: String,
    is_image: bool,
}

enum Container {
    /// A blockquote. `Some(kind)` is a GitHub-style alert (`> [!NOTE]` …), which
    /// recolors the rail and carries a header; `None` is a plain quote.
    Quote(Option<BlockQuoteKind>),
    List(MarkdownList),
}

/// The icon, label, and role style for a GitHub-style alert kind. Icons stay
/// ASCII (guaranteed width 1) so the rail's column math never drifts.
fn alert_meta(theme: &Theme, kind: BlockQuoteKind) -> (char, &'static str, Style) {
    let alert = theme.glyphs().markdown.alert;
    match kind {
        BlockQuoteKind::Note => (alert.note, "NOTE", theme.info()),
        BlockQuoteKind::Tip => (alert.tip, "TIP", theme.success()),
        BlockQuoteKind::Important => (alert.important, "IMPORTANT", theme.primary()),
        BlockQuoteKind::Warning => (alert.warning, "WARNING", theme.warning()),
        BlockQuoteKind::Caution => (alert.caution, "CAUTION", theme.error()),
    }
}

struct MarkdownList {
    next: Option<u64>,
    marker: String,
    first_line: bool,
    in_item: bool,
}

struct MarkdownCodeBlock {
    language: String,
    source: String,
}

struct MarkdownTable {
    alignments: Vec<MarkdownAlignment>,
    rows: Vec<Vec<Line<'static>>>,
    row: Vec<Line<'static>>,
    cell: Line<'static>,
    in_head: bool,
}

impl<'a> MarkdownTerminalRenderer<'a> {
    fn new(
        theme: &'a Theme,
        width: usize,
        show_urls: bool,
        entry_path: Option<&Path>,
        attachments_openable: bool,
    ) -> Self {
        Self {
            theme,
            width: width.max(1),
            show_urls,
            lines: Vec::new(),
            current: Vec::new(),
            styles: vec![Style::default()],
            links: Vec::new(),
            link_targets: Vec::new(),
            link_hits: Vec::new(),
            headings: Vec::new(),
            heading_text: None,
            containers: Vec::new(),
            code: None,
            table: None,
            separate_next_block: false,
            highlight_open: false,
            entry_path: entry_path.map(Path::to_path_buf),
            attachments_openable,
        }
    }

    fn render(mut self, source: &str) -> RenderedChunk {
        let options = MarkdownOptions::ENABLE_TABLES
            | MarkdownOptions::ENABLE_STRIKETHROUGH
            | MarkdownOptions::ENABLE_TASKLISTS
            | MarkdownOptions::ENABLE_GFM
            | MarkdownOptions::ENABLE_HEADING_ATTRIBUTES;
        for event in MarkdownParser::new_ext(source, options) {
            self.event(event);
        }
        self.finish_current(false);
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
        }
        RenderedChunk {
            lines: self.lines,
            links: self.link_hits,
            headings: self.headings,
            link_count: self.link_targets.len(),
        }
    }

    fn event(&mut self, event: MarkdownEvent<'_>) {
        if self.code.is_some() {
            match event {
                MarkdownEvent::End(MarkdownTagEnd::CodeBlock) => self.finish_code_block(),
                MarkdownEvent::Text(text)
                | MarkdownEvent::Code(text)
                | MarkdownEvent::Html(text)
                | MarkdownEvent::InlineHtml(text) => {
                    if let Some(code) = self.code.as_mut() {
                        code.source.push_str(&text);
                    }
                }
                MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak => {
                    if let Some(code) = self.code.as_mut() {
                        code.source.push('\n');
                    }
                }
                _ => {}
            }
            return;
        }

        if self.table.is_some() && self.table_event(&event) {
            return;
        }

        match event {
            MarkdownEvent::Start(tag) => self.start_tag(tag),
            MarkdownEvent::End(tag) => self.end_tag(tag),
            MarkdownEvent::Text(text) => {
                self.capture_link_text(&text);
                self.capture_heading_text(&text);
                self.push_highlighted_text(&text);
            }
            MarkdownEvent::Code(code) => {
                self.capture_link_text(&code);
                self.capture_heading_text(&code);
                self.push_span(&code, self.theme.md_inline_code());
            }
            MarkdownEvent::Html(html) | MarkdownEvent::InlineHtml(html) => {
                self.push_multiline(&html, self.theme.muted());
            }
            MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak => self.finish_current(true),
            MarkdownEvent::Rule => self.render_rule(),
            MarkdownEvent::TaskListMarker(checked) => {
                let glyphs = self.theme.glyphs().markdown.clone();
                let box_glyph = if checked {
                    &glyphs.task_done
                } else {
                    &glyphs.task_todo
                };
                self.push_span(&format!("{box_glyph} "), self.theme.muted())
            }
            MarkdownEvent::FootnoteReference(label) => {
                self.push_span(&format!("[{label}]"), self.theme.md_link());
            }
            MarkdownEvent::InlineMath(math) => self.push_span(&math, self.theme.md_inline_code()),
            MarkdownEvent::DisplayMath(math) => {
                self.begin_block();
                self.push_multiline(&math, self.theme.md_code());
                self.finish_current(false);
                self.separate_next_block = true;
            }
        }
    }

    fn start_tag(&mut self, tag: MarkdownTag<'_>) {
        match tag {
            MarkdownTag::Paragraph => self.start_paragraph(),
            MarkdownTag::Heading { level, .. } => {
                self.begin_block();
                self.heading_text = Some(String::new());
                let style = match level {
                    HeadingLevel::H1 => self.theme.md_heading(),
                    HeadingLevel::H2 => self.theme.md_heading2(),
                    _ => self.theme.md_subheading(),
                };
                self.push_style(style);
                self.push_span(&format!("{} ", "#".repeat(level as usize)), style);
            }
            MarkdownTag::BlockQuote(kind) => {
                self.begin_block();
                self.containers.push(Container::Quote(kind));
                if let Some(kind) = kind {
                    let (icon, label, style) = alert_meta(self.theme, kind);
                    self.push_span(
                        &format!("{icon} {label}"),
                        style.add_modifier(Modifier::BOLD),
                    );
                    self.finish_current(false);
                }
            }
            MarkdownTag::CodeBlock(kind) => {
                self.begin_block();
                let language = match kind {
                    CodeBlockKind::Fenced(language) => language.into_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some(MarkdownCodeBlock {
                    language,
                    source: String::new(),
                });
            }
            MarkdownTag::List(start) => {
                self.begin_block();
                self.containers.push(Container::List(MarkdownList {
                    next: start,
                    marker: String::new(),
                    first_line: false,
                    in_item: false,
                }));
            }
            MarkdownTag::Item => self.start_list_item(),
            MarkdownTag::Table(alignments) => {
                self.begin_block();
                self.table = Some(MarkdownTable {
                    alignments,
                    rows: Vec::new(),
                    row: Vec::new(),
                    cell: Line::default(),
                    in_head: false,
                });
            }
            MarkdownTag::Emphasis => self.push_style(Style::new().italic()),
            MarkdownTag::Strong => self.push_style(Style::new().bold()),
            MarkdownTag::Strikethrough => {
                self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT));
            }
            MarkdownTag::Link { dest_url, .. } => self.start_link(dest_url.into_string(), false),
            MarkdownTag::Image { dest_url, .. } => self.start_link(dest_url.into_string(), true),
            // An HTML block renders as its raw text; start a new block so the
            // blank-line separator a preceding paragraph owes is emitted before
            // it, rather than gluing the two together.
            MarkdownTag::HtmlBlock => self.begin_block(),
            MarkdownTag::FootnoteDefinition(_)
            | MarkdownTag::DefinitionList
            | MarkdownTag::DefinitionListTitle
            | MarkdownTag::DefinitionListDefinition
            | MarkdownTag::TableHead
            | MarkdownTag::TableRow
            | MarkdownTag::TableCell
            | MarkdownTag::MetadataBlock(_)
            | MarkdownTag::Superscript
            | MarkdownTag::Subscript => {}
        }
    }

    fn end_tag(&mut self, tag: MarkdownTagEnd) {
        match tag {
            MarkdownTagEnd::Paragraph => {
                self.finish_current(false);
                self.separate_next_block = true;
            }
            MarkdownTagEnd::Heading(_) => {
                self.pop_style();
                // Record the anchor against the heading's first display line —
                // `self.lines.len()` is that index because `finish_current` below
                // is what pushes the heading's rows. The slug comes from the whole
                // title, so it stays correct even when the heading wraps.
                if let Some(text) = self.heading_text.take() {
                    let anchor = heading_anchor(text.trim());
                    if !anchor.is_empty() {
                        self.headings.push(ReaderHeading {
                            anchor,
                            line: self.lines.len(),
                        });
                    }
                }
                self.finish_current(false);
                self.separate_next_block = true;
            }
            MarkdownTagEnd::BlockQuote(_) => {
                self.finish_current(false);
                self.containers.pop();
                self.separate_next_block = true;
            }
            MarkdownTagEnd::List(_) => {
                self.finish_current(false);
                self.containers.pop();
                self.separate_next_block = !self.in_list();
            }
            MarkdownTagEnd::Item => {
                self.finish_current(false);
                if let Some(list) = self.current_list_mut() {
                    list.in_item = false;
                }
                self.separate_next_block = false;
            }
            MarkdownTagEnd::Emphasis | MarkdownTagEnd::Strong | MarkdownTagEnd::Strikethrough => {
                self.pop_style()
            }
            MarkdownTagEnd::Link | MarkdownTagEnd::Image => {
                self.pop_style();
                if let Some((target, text, _id)) = self.links.pop()
                    && self.show_urls
                    && text.trim() != target.trim()
                {
                    // The name (tagged with its link id, so it is the clickable
                    // region) stays `md_link`; the untagged target trails it in the
                    // faint secondary style. When URLs are hidden the trailer is
                    // skipped entirely, so wrapping never accounts for it.
                    self.push_span(" (", self.theme.muted());
                    self.push_span(&target, self.theme.muted());
                    self.push_span(")", self.theme.muted());
                }
            }
            MarkdownTagEnd::CodeBlock | MarkdownTagEnd::Table => {}
            MarkdownTagEnd::HtmlBlock
            | MarkdownTagEnd::FootnoteDefinition
            | MarkdownTagEnd::TableHead
            | MarkdownTagEnd::TableRow
            | MarkdownTagEnd::TableCell
            | MarkdownTagEnd::MetadataBlock(_)
            | MarkdownTagEnd::DefinitionList
            | MarkdownTagEnd::DefinitionListTitle
            | MarkdownTagEnd::DefinitionListDefinition
            | MarkdownTagEnd::Superscript
            | MarkdownTagEnd::Subscript => {}
        }
    }

    fn start_link(&mut self, target: String, is_image: bool) {
        let id = self.link_targets.len();
        self.link_targets.push(LinkTarget {
            target: target.clone(),
            is_image,
        });
        self.links.push((target, String::new(), id));
        self.push_style(self.theme.md_link());
    }

    fn start_paragraph(&mut self) {
        // A blank separator before this block (a second paragraph in the item, or
        // a block after one) is driven by `separate_next_block` through
        // `begin_block`, which runs before any nested container is entered — so the
        // separator sits at the list's indent and never inherits a blockquote rail.
        self.begin_block();
    }

    fn start_list_item(&mut self) {
        self.finish_current(false);
        let bullet = self.theme.glyphs().markdown.bullet;
        let Some(list) = self.current_list_mut() else {
            return;
        };
        list.marker = match list.next {
            Some(number) => {
                list.next = Some(number.saturating_add(1));
                format!("{number}. ")
            }
            None => format!("{bullet} "),
        };
        list.first_line = true;
        list.in_item = true;
        self.separate_next_block = false;
    }

    fn begin_block(&mut self) {
        self.finish_current(false);
        if self.separate_next_block && !self.lines.is_empty() {
            self.emit_blank_line();
        }
        self.separate_next_block = false;
        // A `==highlight==` never spans block boundaries; without this reset an
        // unpaired `==` (e.g. a lone one in prose) would leak the highlight style
        // into every following block. The editor highlighter is likewise
        // per-line-conservative, so this keeps reader and editor in agreement.
        self.highlight_open = false;
    }

    fn push_style(&mut self, style: Style) {
        self.styles.push(self.current_style().patch(style));
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn current_style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_span(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        if let Some(table) = self.table.as_mut() {
            // Table cells carry no link semantics; keep the plain ratatui merge.
            if let Some(span) = table.cell.spans.last_mut()
                && span.style == style
            {
                span.content.to_mut().push_str(text);
            } else {
                table.cell.spans.push(Span::styled(text.to_string(), style));
            }
            return;
        }
        let link = self.current_link();
        if let Some(span) = self.current.last_mut()
            && span.style == style
            && span.link == link
        {
            span.content.push_str(text);
        } else {
            self.current.push(RichSpan {
                content: text.to_string(),
                style,
                link,
            });
        }
    }

    /// The id of the innermost open link/image, tagging characters emitted while
    /// it is open so wrapping can recover the clickable region.
    fn current_link(&self) -> Option<usize> {
        self.links.last().map(|(_, _, id)| *id)
    }

    /// Accumulate an open link's visible text so the closing tag can compare it
    /// against the target (autolinks render the URL as their own name).
    fn capture_link_text(&mut self, text: &str) {
        if let Some((_, name, _)) = self.links.last_mut() {
            name.push_str(text);
        }
    }

    /// Accumulate the open heading's visible text for its anchor slug.
    fn capture_heading_text(&mut self, text: &str) {
        if let Some(heading) = self.heading_text.as_mut() {
            heading.push_str(text);
        }
    }

    fn push_multiline(&mut self, text: &str, style: Style) {
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                self.finish_current(true);
            }
            self.push_span(line, style);
        }
    }

    fn push_highlighted_text(&mut self, text: &str) {
        let mut rest = text;
        while let Some(index) = rest.find("==") {
            self.push_span(&rest[..index], self.highlight_style());
            self.highlight_open = !self.highlight_open;
            rest = &rest[index + 2..];
        }
        self.push_span(rest, self.highlight_style());
    }

    fn highlight_style(&self) -> Style {
        if self.highlight_open {
            self.current_style().patch(self.theme.md_highlight())
        } else {
            self.current_style()
        }
    }

    fn finish_current(&mut self, force: bool) {
        if !force && self.current.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.current);
        self.emit_wrapped_line(spans, None, false);
    }

    fn emit_blank_line(&mut self) {
        self.emit_wrapped_line(Vec::new(), None, false);
    }

    /// The innermost open list, ignoring any blockquotes nested inside it.
    fn current_list_mut(&mut self) -> Option<&mut MarkdownList> {
        self.containers
            .iter_mut()
            .rev()
            .find_map(|container| match container {
                Container::List(list) => Some(list),
                Container::Quote(_) => None,
            })
    }

    fn in_list(&self) -> bool {
        self.containers
            .iter()
            .any(|container| matches!(container, Container::List(_)))
    }

    fn container_prefix(&mut self, markers: bool) -> Line<'static> {
        let mut spans = Vec::new();
        for container in &mut self.containers {
            match container {
                Container::Quote(kind) => {
                    let style = kind.map_or_else(
                        || self.theme.md_blockquote(),
                        |kind| alert_meta(self.theme, kind).2,
                    );
                    spans.push(Span::styled(
                        self.theme.glyphs().markdown.quote_rail.clone(),
                        style,
                    ));
                }
                Container::List(list) if list.in_item => {
                    if markers && list.first_line {
                        spans.push(Span::styled(list.marker.clone(), self.theme.muted()));
                        list.first_line = false;
                    } else {
                        spans.push(Span::raw(" ".repeat(list.marker.width())));
                    }
                }
                Container::List(_) => {}
            }
        }
        Line::from(spans)
    }

    fn emit_wrapped_line(&mut self, spans: Vec<RichSpan>, rail: Option<&str>, hard_wrap: bool) {
        let mut first_prefix = self.container_prefix(true);
        if let Some(rail) = rail {
            first_prefix
                .spans
                .push(Span::styled(rail.to_string(), self.theme.md_blockquote()));
        }
        let available = self.width.saturating_sub(first_prefix.width()).max(1);
        let wrapped = wrap_rich(spans, available, hard_wrap);
        for (index, content) in wrapped.into_iter().enumerate() {
            let mut prefix = if index == 0 {
                first_prefix.clone()
            } else {
                let mut prefix = self.container_prefix(false);
                if let Some(rail) = rail {
                    prefix
                        .spans
                        .push(Span::styled(rail.to_string(), self.theme.md_blockquote()));
                }
                prefix
            };
            // A link name that straddles a wrap boundary records one hit segment
            // per display line; the prefix (blockquote rail, list indent) shifts
            // every content column, so hit columns include it — matching the
            // absolute body-line columns click hit-testing compares against.
            let prefix_width = prefix.width();
            for (start, end, id) in content.links {
                let link = &self.link_targets[id];
                let openable = is_openable_link(&link.target)
                    || (!link.is_image
                        && is_openable_attachment(
                            &link.target,
                            self.attachments_openable,
                            self.entry_path.as_deref(),
                        ));
                if openable {
                    self.link_hits.push(ReaderLinkHit {
                        line: self.lines.len(),
                        start: prefix_width.saturating_add(start),
                        end: prefix_width.saturating_add(end),
                        target: link.target.clone(),
                        group: id,
                    });
                }
            }
            prefix.spans.extend(content.spans);
            self.lines.push(prefix);
        }
    }

    fn render_rule(&mut self) {
        self.begin_block();
        let prefix_width = self.container_prefix(false).width();
        let width = self.width.saturating_sub(prefix_width).max(1);
        let glyph = self
            .theme
            .glyphs()
            .borders
            .line_set()
            .horizontal
            .to_string();
        self.emit_wrapped_line(
            vec![RichSpan {
                content: glyph.repeat(width),
                style: self.theme.muted(),
                link: None,
            }],
            None,
            true,
        );
        self.separate_next_block = true;
    }

    fn finish_code_block(&mut self) {
        let Some(code) = self.code.take() else {
            return;
        };
        // GitHub info strings can carry attributes after the language, comma- or
        // space-separated (```rust,ignore); take just the language token.
        let language = code
            .language
            .split([' ', '\t', ','])
            .next()
            .unwrap_or_default();
        let t = self.theme;
        let markdown = &t.glyphs().markdown;
        let header = if language.is_empty() {
            markdown.code_top.clone()
        } else {
            format!("{} {language}", markdown.code_top)
        };
        self.emit_wrapped_line(
            vec![RichSpan {
                content: header,
                style: t.md_blockquote(),
                link: None,
            }],
            None,
            true,
        );
        let source = code.source.trim_end_matches('\n').replace('\t', "    ");
        let highlighted = crate::tui::syntax_highlight::highlight(t, language, &source);
        let code_lines = highlighted.unwrap_or_else(|| {
            source
                .split('\n')
                .map(|line| Line::from(Span::styled(line.to_string(), t.md_code())))
                .collect()
        });
        let code_rail = markdown.code_rail.as_str();
        for line in code_lines {
            self.emit_wrapped_line(rich_from_line(line), Some(code_rail), true);
        }
        self.emit_wrapped_line(
            vec![RichSpan {
                content: markdown.code_bottom.clone(),
                style: t.md_blockquote(),
                link: None,
            }],
            None,
            true,
        );
        self.separate_next_block = true;
    }
}

/// Whether a link target is worth making clickable — external URLs and in-page
/// heading anchors. Relative asset paths stay styled but non-interactive here;
/// stored attachments are handled by [`is_openable_attachment`].
fn is_openable_link(text: &str) -> bool {
    text.starts_with('#')
        || text.starts_with("https://")
        || text.starts_with("http://")
        || text.starts_with("mailto:")
}

/// Whether `target` is a link into `entry_path`'s own asset folder that should be
/// opened in the OS default app. Only enabled for plaintext entries — an
/// encrypted entry's assets live on disk as `.age` and can't be opened directly,
/// so their links stay inert (no hover, no click).
fn is_openable_attachment(
    target: &str,
    attachments_openable: bool,
    entry_path: Option<&Path>,
) -> bool {
    attachments_openable
        && entry_path
            .and_then(|path| notema_storage::stored_asset_reference_for(path, target))
            .is_some()
}

/// Slugify heading text into a GitHub-style anchor: lowercased, alphanumerics /
/// `_` / `-` kept, whitespace runs collapsed to single `-`, edges trimmed.
fn heading_anchor(text: &str) -> String {
    let mut anchor = String::with_capacity(text.len());
    let mut separator = false;
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' || character == '-' {
            if separator && !anchor.is_empty() && !anchor.ends_with('-') {
                anchor.push('-');
            }
            separator = false;
            anchor.push(character);
        } else if character.is_whitespace() {
            separator = true;
        }
    }
    anchor.trim_matches('-').to_string()
}

#[cfg(test)]
mod wrap_tests {
    use super::*;
    use ratatui::style::Modifier;

    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// Render just the display lines (URLs shown) for the many tests that only
    /// assert on rendered text.
    fn render_lines(source: &str, width: usize) -> Vec<Line<'static>> {
        render_text_chunk(&Theme::terminal_default(), source, width, true, None, false).lines
    }

    #[test]
    fn wraps_at_words_instead_of_the_panel_edge() {
        let wrapped = wrap_line(Line::from("alpha beta"), 7);

        assert_eq!(
            wrapped.iter().map(text).collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
    }

    #[test]
    fn punctuation_stays_with_a_word_across_style_boundaries() {
        let emphasis = Style::new().add_modifier(Modifier::ITALIC);
        let wrapped = wrap_line(
            Line::from(vec![
                Span::raw("Hello "),
                Span::styled("world", emphasis),
                Span::raw("! again."),
            ]),
            13,
        );

        assert_eq!(
            wrapped.iter().map(text).collect::<Vec<_>>(),
            ["Hello world!", "again."]
        );
        assert_eq!(wrapped[0].spans[1].style, emphasis);
        assert_eq!(wrapped[0].spans[1].content, "world");
        assert_eq!(wrapped[0].spans[2].content, "!");
    }

    #[test]
    fn rendered_markdown_does_not_orphan_punctuation_after_emphasis() {
        let lines = render_lines("Hello **world**! again.", 13);
        let visible: Vec<String> = lines
            .iter()
            .map(text)
            .filter(|line| !line.is_empty())
            .collect();

        assert_eq!(visible, ["Hello world!", "again."]);
    }

    #[test]
    fn fenced_code_uses_the_previous_open_rail_frame() {
        let lines = render_lines("```rust\nfn main() {}\n```", 40);
        let visible: Vec<String> = lines.iter().map(text).collect();

        assert_eq!(visible, ["╭─ rust", "│ fn main() {}", "╰─"]);
        assert_eq!(
            lines[0].spans[0].style,
            Theme::terminal_default().md_blockquote()
        );
        assert_eq!(
            lines[1].spans[0].style,
            Theme::terminal_default().md_blockquote()
        );
        assert_eq!(
            lines[2].spans[0].style,
            Theme::terminal_default().md_blockquote()
        );
    }

    #[test]
    fn every_wrapped_code_row_keeps_the_frame_rail() {
        let lines = render_lines("```\nabcdefgh\n```", 8);
        let visible: Vec<String> = lines.iter().map(text).collect();

        assert_eq!(visible, ["╭─", "│ abcdef", "│ gh", "╰─"]);
    }

    #[test]
    fn gfm_table_uses_the_shared_theme_grid() {
        let source = concat!(
            "| Hi  | sdf | sdf | s   | s   |\n",
            "|-----|-----|-----|-----|-----|\n",
            "| sdf | sfd | sdf | sdf | sdf |\n",
            "| g   | g   | ss  | h   | r   |\n",
        );
        let lines = render_lines(source, 50);
        let visible: Vec<String> = lines.iter().map(text).collect();

        assert!(
            visible[0].starts_with(
                Theme::terminal_default()
                    .glyphs()
                    .borders
                    .line_set()
                    .top_left
            )
        );
        assert!(visible[1].contains("Hi"));
        assert!(visible[2].contains(Theme::terminal_default().glyphs().borders.line_set().cross));
        assert!(
            visible.last().unwrap().starts_with(
                Theme::terminal_default()
                    .glyphs()
                    .borders
                    .line_set()
                    .bottom_left
            )
        );
        assert!(!visible.iter().any(|line| line.contains("-----")));
    }

    #[test]
    fn lists_are_flush_left_with_hanging_continuations() {
        let source = concat!(
            "1.  This is a list item with two paragraphs. Lorem ipsum dolor\n",
            "    sit amet, consectetuer adipiscing elit.\n",
            "\n",
            "    Vestibulum enim wisi, viverra nec.\n",
            "\n",
            "2.  Suspendisse id sem.\n",
        );
        let visible: Vec<String> = render_lines(source, 72).iter().map(text).collect();

        assert_eq!(
            visible[0],
            "1. This is a list item with two paragraphs. Lorem ipsum dolor"
        );
        assert_eq!(visible[1], "   sit amet, consectetuer adipiscing elit.");
        assert_eq!(visible[2], "   ");
        assert_eq!(visible[3], "   Vestibulum enim wisi, viverra nec.");
        assert_eq!(visible[4], "2. Suspendisse id sem.");

        let bullets: Vec<String> = render_lines("* Hello\n* Test", 30)
            .iter()
            .map(text)
            .collect();
        assert_eq!(bullets, ["- Hello", "- Test"]);
    }

    #[test]
    fn blockquote_in_a_list_item_follows_the_list_indent() {
        let source = "*   A list item with a blockquote:\n\n    > This is a blockquote\n    > inside a list item.";
        let visible: Vec<String> = render_lines(source, 40).iter().map(text).collect();

        assert_eq!(visible[0], "- A list item with a blockquote:");
        // The separator between the item text and the blockquote sits at the list
        // indent only — the vertical rail must not leak onto the blank line.
        assert_eq!(visible[1].trim_end(), "");
        assert!(!visible[1].contains('│'), "{visible:?}");
        assert!(
            visible
                .iter()
                .any(|line| line == "  │ This is a blockquote"),
            "{visible:?}"
        );
    }

    #[test]
    fn gfm_alert_renders_a_header_and_recolors_the_rail() {
        let lines = render_lines("> [!WARNING]\n> Critical content here.", 40);
        let visible: Vec<String> = lines.iter().map(text).collect();

        assert_eq!(visible[0], "│ ! WARNING");
        assert_eq!(visible[1], "│ Critical content here.");
        // The parser consumed the marker — it never leaks into the body.
        assert!(!visible.iter().any(|line| line.contains("[!WARNING]")));
        // Rail and header both carry the warning role; the header is bold.
        assert_eq!(lines[0].spans[0].style, Theme::terminal_default().warning());
        assert_eq!(
            lines[0].spans[1].style,
            Theme::terminal_default()
                .warning()
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(lines[1].spans[0].style, Theme::terminal_default().warning());
    }

    #[test]
    fn every_gfm_alert_kind_gets_its_label() {
        for (marker, header) in [
            ("NOTE", "i NOTE"),
            ("TIP", "* TIP"),
            ("IMPORTANT", "! IMPORTANT"),
            ("WARNING", "! WARNING"),
            ("CAUTION", "! CAUTION"),
        ] {
            let visible: Vec<String> = render_lines(&format!("> [!{marker}]\n> body"), 40)
                .iter()
                .map(text)
                .collect();
            assert_eq!(visible[0], format!("│ {header}"));
            assert_eq!(visible[1], "│ body");
        }
    }

    #[test]
    fn plain_blockquote_keeps_its_neutral_rail_and_no_header() {
        let lines = render_lines("> just a quote", 40);
        let visible: Vec<String> = lines.iter().map(text).collect();

        assert_eq!(visible, ["│ just a quote"]);
        assert_eq!(
            lines[0].spans[0].style,
            Theme::terminal_default().md_blockquote()
        );
    }

    #[test]
    fn gfm_alert_body_wraps_under_a_continuing_rail() {
        let visible: Vec<String> = render_lines("> [!NOTE]\n> alpha beta gamma delta", 10)
            .iter()
            .map(text)
            .collect();

        assert_eq!(visible[0], "│ i NOTE");
        assert!(visible[1..].iter().all(|line| line.starts_with("│ ")));
        assert!(visible.iter().any(|line| line.contains("alpha")));
        assert!(visible.iter().any(|line| line.contains("delta")));
    }

    #[test]
    fn indented_code_preserves_lines_and_receives_a_frame() {
        let source = "Here is code:\n\n    tell application \"Foo\"\n        beep\n    end tell\n";
        let visible: Vec<String> = render_lines(source, 40).iter().map(text).collect();

        assert!(visible.windows(5).any(|lines| lines
            == [
                "╭─",
                "│ tell application \"Foo\"",
                "│     beep",
                "│ end tell",
                "╰─",
            ]));
    }

    #[test]
    fn nested_quotes_use_composable_vertical_rails() {
        let visible: Vec<String> = render_lines(
            "> ## This is a header.\n>\n> 1. First\n> 2. Second\n>\n> Here's code:\n>\n>     return value\n",
            40,
        )
        .iter()
        .map(text)
        .collect();

        assert!(visible.iter().any(|line| line == "│ ## This is a header."));
        assert!(visible.iter().any(|line| line == "│ 1. First"));
        assert!(visible.iter().any(|line| line == "│ ╭─"));
        assert!(visible.iter().any(|line| line == "│ │ return value"));

        let nested: Vec<String> = render_lines("> simple quote\n>> second level quote", 40)
            .iter()
            .map(text)
            .collect();
        assert!(nested.iter().any(|line| line == "│ simple quote"));
        assert!(nested.iter().any(|line| line == "│ │ second level quote"));
    }

    #[test]
    fn thematic_breaks_and_highlights_have_terminal_styles() {
        for marker in ["***", "---", "___", "_____________________________________"] {
            let lines = render_lines(marker, 12);
            assert_eq!(lines.len(), 1, "{marker}");
            assert_eq!(lines[0].width(), 12, "{marker}");
            assert_eq!(
                lines[0].spans[0].style,
                Theme::terminal_default().muted(),
                "{marker}"
            );
        }

        let lines = render_lines("These are ==very important words==.", 40);
        let marked = lines[0]
            .spans
            .iter()
            .find(|span| span.content == "very important words")
            .unwrap();
        assert!(marked.style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn link_names_stay_blue_with_a_faint_target_trailer() {
        let chunk = render_text_chunk(
            &Theme::terminal_default(),
            "See [the docs](https://example.com) now.",
            60,
            true,
            None,
            false,
        );
        let spans = &chunk.lines[0].spans;

        let name = spans
            .iter()
            .find(|span| span.content == "the docs")
            .unwrap();
        assert_eq!(name.style, Theme::terminal_default().md_link());
        let trailer = spans
            .iter()
            .find(|span| span.content == " (https://example.com)")
            .unwrap();
        assert_eq!(trailer.style, Theme::terminal_default().muted());

        // The name — not the trailer — is the recorded clickable region.
        assert_eq!(chunk.links.len(), 1);
        assert_eq!(chunk.links[0].target, "https://example.com");
        assert_eq!((chunk.links[0].start, chunk.links[0].end), (4, 12));
    }

    #[test]
    fn autolinks_drop_the_redundant_target_trailer() {
        let chunk = render_text_chunk(
            &Theme::terminal_default(),
            "<https://example.com>",
            60,
            true,
            None,
            false,
        );
        let rendered: String = chunk.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(rendered, "https://example.com");
        // The bare URL name is itself the clickable region.
        assert_eq!(chunk.links.len(), 1);
        assert_eq!(chunk.links[0].target, "https://example.com");
    }

    #[test]
    fn hard_wraps_only_tokens_wider_than_the_panel() {
        let wrapped = wrap_line(Line::from("abcdefgh"), 4);

        assert_eq!(
            wrapped.iter().map(text).collect::<Vec<_>>(),
            ["abcd", "efgh"]
        );
    }

    #[test]
    fn wrapping_uses_terminal_cell_width() {
        let wrapped = wrap_line(Line::from("ab 界!"), 4);

        assert_eq!(wrapped.iter().map(text).collect::<Vec<_>>(), ["ab", "界!"]);
    }
}
