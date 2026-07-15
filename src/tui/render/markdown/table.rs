use super::*;

impl MarkdownTerminalRenderer<'_> {
    pub(super) fn table_event(&mut self, event: &MarkdownEvent<'_>) -> bool {
        match event {
            MarkdownEvent::Start(MarkdownTag::TableHead) => {
                self.table.as_mut().expect("table exists").in_head = true;
            }
            MarkdownEvent::End(MarkdownTagEnd::TableHead) => {
                let table = self.table.as_mut().expect("table exists");
                table.in_head = false;
                if !table.row.is_empty() {
                    table.rows.push(std::mem::take(&mut table.row));
                }
            }
            MarkdownEvent::Start(MarkdownTag::TableRow) => {}
            MarkdownEvent::End(MarkdownTagEnd::TableRow) => {
                let table = self.table.as_mut().expect("table exists");
                table.rows.push(std::mem::take(&mut table.row));
            }
            MarkdownEvent::Start(MarkdownTag::TableCell) => {}
            MarkdownEvent::End(MarkdownTagEnd::TableCell) => {
                let table = self.table.as_mut().expect("table exists");
                let mut cell = std::mem::take(&mut table.cell);
                if table.in_head {
                    cell = cell.patch_style(Style::new().bold());
                }
                table.row.push(cell);
            }
            MarkdownEvent::End(MarkdownTagEnd::Table) => self.finish_table(),
            MarkdownEvent::Start(MarkdownTag::Emphasis) => self.push_style(Style::new().italic()),
            MarkdownEvent::End(MarkdownTagEnd::Emphasis) => self.pop_style(),
            MarkdownEvent::Start(MarkdownTag::Strong) => self.push_style(Style::new().bold()),
            MarkdownEvent::End(MarkdownTagEnd::Strong) => self.pop_style(),
            MarkdownEvent::Start(MarkdownTag::Strikethrough) => {
                self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT));
            }
            MarkdownEvent::End(MarkdownTagEnd::Strikethrough) => self.pop_style(),
            MarkdownEvent::Code(code) => self.push_span(code, self.theme.md_inline_code()),
            MarkdownEvent::Text(text) => self.push_highlighted_text(text),
            MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak => {
                self.push_span(" ", self.current_style());
            }
            _ => {}
        }
        true
    }

    fn finish_table(&mut self) {
        let Some(table_data) = self.table.take() else {
            return;
        };
        if table_data.rows.is_empty() {
            return;
        }
        let columns = table_data
            .rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or_default();
        if columns == 0 {
            return;
        }
        let prefix_width = self.container_prefix(false).width();
        let available = self.width.saturating_sub(prefix_width);
        let overhead = columns.saturating_mul(3).saturating_add(1);
        if available <= overhead.saturating_add(columns) {
            self.render_stacked_table(table_data.rows);
            self.separate_next_block = true;
            return;
        }
        let content_width = available - overhead;
        let mut widths = vec![1usize; columns];
        for row in &table_data.rows {
            for (column, cell) in row.iter().enumerate() {
                widths[column] = widths[column].max(cell.width());
            }
        }
        while widths.iter().sum::<usize>() > content_width {
            let Some((column, _)) = widths.iter().enumerate().max_by_key(|(_, width)| **width)
            else {
                break;
            };
            if widths[column] <= 1 {
                break;
            }
            widths[column] -= 1;
        }

        let border = super::super::table::themed_border_style(self.theme);
        self.emit_table_line(super::super::table::themed_rule(
            self.theme,
            &widths,
            super::super::table::RulePos::Top,
            border,
            border,
        ));
        for (row_index, row) in table_data.rows.iter().enumerate() {
            let wrapped: Vec<Vec<Line<'static>>> = (0..columns)
                .map(|column| {
                    row.get(column).cloned().map_or_else(
                        || vec![Line::default()],
                        |cell| wrap_line(cell, widths[column]),
                    )
                })
                .collect();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for line_index in 0..height {
                let mut spans = vec![super::super::table::themed_border(self.theme)];
                for column in 0..columns {
                    let mut cell = wrapped[column]
                        .get(line_index)
                        .cloned()
                        .unwrap_or_default()
                        .spans;
                    let used = cell.iter().map(Span::width).sum::<usize>();
                    let padding = widths[column].saturating_sub(used);
                    let (left, right) = match table_data
                        .alignments
                        .get(column)
                        .copied()
                        .unwrap_or(MarkdownAlignment::None)
                    {
                        MarkdownAlignment::Right => (padding, 0),
                        MarkdownAlignment::Center => (padding / 2, padding - padding / 2),
                        MarkdownAlignment::None | MarkdownAlignment::Left => (0, padding),
                    };
                    if left > 0 {
                        cell.insert(0, Span::raw(" ".repeat(left)));
                    }
                    if right > 0 {
                        cell.push(Span::raw(" ".repeat(right)));
                    }
                    super::super::table::themed_push_cell_spans(self.theme, &mut spans, cell);
                }
                self.emit_table_line(Line::from(spans));
            }
            if row_index == 0 && table_data.rows.len() > 1 {
                self.emit_table_line(super::super::table::themed_rule(
                    self.theme,
                    &widths,
                    super::super::table::RulePos::Mid,
                    border,
                    border,
                ));
            } else if row_index + 1 < table_data.rows.len() {
                self.emit_table_line(super::super::table::themed_rule(
                    self.theme,
                    &widths,
                    super::super::table::RulePos::Row,
                    border,
                    super::super::table::themed_faint_rule_style(self.theme),
                ));
            }
        }
        self.emit_table_line(super::super::table::themed_rule(
            self.theme,
            &widths,
            super::super::table::RulePos::Bottom,
            border,
            border,
        ));
        self.separate_next_block = true;
    }

    fn render_stacked_table(&mut self, rows: Vec<Vec<Line<'static>>>) {
        let headers = rows.first().cloned().unwrap_or_default();
        for (row_index, row) in rows.iter().skip(1).enumerate() {
            if row_index > 0 {
                self.emit_blank_line();
            }
            for (column, value) in row.iter().enumerate() {
                let mut line = headers.get(column).cloned().unwrap_or_default();
                if !line.spans.is_empty() {
                    line.spans.push(Span::styled(": ", self.theme.muted()));
                }
                line.spans.extend(value.spans.clone());
                self.emit_wrapped_line(rich_from_line(line), None, false);
            }
        }
    }

    fn emit_table_line(&mut self, mut line: Line<'static>) {
        let mut prefix = self.container_prefix(true);
        prefix.spans.append(&mut line.spans);
        self.lines.push(prefix);
    }
}
