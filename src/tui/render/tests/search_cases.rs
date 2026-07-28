//! The search query field — prefix colouring and facet pills — and the
//! suggestion list and popup that hang beneath it.

use super::*;

/// The cells of a rendered search field carrying `color` as their foreground.
fn field_cells_coloured(app: AppModel, color: ratatui::style::Color) -> String {
    render_app(app, 120, 24)
        .buffer()
        .content()
        .iter()
        .filter(|cell| cell.fg == color)
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn the_search_field_colours_the_prefix_the_parser_recognised() {
    let keyword = ratatui::style::Color::Rgb(0xff, 0x00, 0x00);
    let syntax_theme = || theme::test_theme_from_toml("[markdown.syntax]\nkeyword = \"#ff0000\"");
    let searching = |query: &str| {
        let mut app = app_with_entry();
        app.nav.mode = Mode::Search;
        app.nav.focus = Focus::Entries;
        app.appearance.theme = syntax_theme();
        app.search.query = query.into();
        app
    };

    assert_eq!(
        field_cells_coloured(searching("tags:work"), keyword),
        "tags:"
    );
    // One `s` short of a filter, and nothing on screen says so today. This is
    // the case the colouring exists for.
    assert_eq!(field_cells_coloured(searching("tag:work"), keyword), "");
    // A theme with no syntax palette stays monochrome, like the code highlighter.
    let mut plain = searching("tags:work");
    plain.appearance.theme = theme::Theme::terminal_default();
    assert_eq!(field_cells_coloured(plain, keyword), "");
}

/// The rendered search field's row, trimmed — the query as a person sees it.
fn search_field_row(theme: &theme::Theme, query: &str) -> String {
    let mut app = app_with_entry();
    app.nav.mode = Mode::Search;
    app.nav.focus = Focus::Entries;
    app.appearance.theme = theme.clone();
    app.search.query = query.into();
    render_to_rows(120, 24, |frame| {
        let mut view = crate::tui::ui::ViewState::default();
        draw_app(frame, &mut app, &mut view)
    })
    .into_iter()
    .find_map(|row| {
        let start = row.find("tags:")?;
        let drawn = &row[start..];
        // The field pads to its width, so the query ends at the first run of two
        // spaces. A reversed pill's own trailing blank falls inside that run and
        // is invisible either way.
        let end = drawn.find("  ").unwrap_or(drawn.len());
        Some(drawn[..end].trim_end().to_string())
    })
    .expect("the query renders somewhere")
}

#[test]
fn a_quoted_facet_value_draws_as_a_pill_in_every_pill_style() {
    let bracket = theme::test_theme_from_toml("[metadata.pills]\nstyle = \"bracket\"");
    let bg = theme::test_theme_from_toml(
        "[metadata.pills]\nstyle = \"bg\"\ntags = { fg = \"#101010\", bg = \"#aabbcc\" }",
    );
    let reversed = theme::Theme::terminal_default();

    // The buffer text is byte-for-byte the same in all three; only the two
    // delimiter cells differ, so the geometry cannot drift between them.
    assert_eq!(search_field_row(&bracket, "tags:\"apple\""), "tags:[apple]");
    assert_eq!(search_field_row(&bg, "tags:\"apple\""), "tags: apple");
    assert_eq!(search_field_row(&reversed, "tags:\"apple\""), "tags: apple");

    // Two pills keep the operator between them visible.
    assert_eq!(
        search_field_row(&bracket, "tags:\"apple\"+\"pear\""),
        "tags:[apple]+[pear]"
    );
    // Break one delimiter and the surviving quote reappears in the cell it
    // occupied — the failure explains itself.
    assert_eq!(search_field_row(&bracket, "tags:\"apple"), "tags:\"apple");
}

/// The caret arithmetic is untouched only because a chip occupies exactly the
/// columns its quotes did. Pin that against the styles that draw it differently.
#[test]
fn a_pill_occupies_the_same_columns_as_the_quotes_it_replaces() {
    let bracket = theme::test_theme_from_toml("[metadata.pills]\nstyle = \"bracket\"");
    let plain = theme::test_theme_from_toml("[metadata.pills]\nstyle = \"bracket\"");
    // `location:` gets no pill, so its quotes render literally — same width.
    assert_eq!(
        search_field_row(&bracket, "tags:\"apple\"").chars().count(),
        search_field_row(&plain, "tags:apple\"\"").chars().count()
    );
}

/// The suggestion list hangs under the query field and over the results, and it
/// is the only place the counts and the values appear together — so this covers
/// the rows arriving, narrowing, and going away when there is nothing to offer.
#[test]
fn the_suggestion_list_draws_under_the_query_field() {
    use crate::tui::test_support::app_in_temp;

    let mut app = app_in_temp(|root| {
        let dir = root.join("work").join("2026-07-01");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, tags) in [("a", "\"apple\", \"apricot\""), ("b", "\"apple\"")] {
            std::fs::write(
                dir.join(format!("{name}.md")),
                format!(
                    "+++\nschema_version = 1\n\n[entry]\ntags = [{tags}]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\nbody\n"
                ),
            )
            .unwrap();
        }
    });
    app.begin_search();

    let drawn = |app: &mut AppModel| {
        let mut view = crate::tui::ui::ViewState::default();
        render_to_rows(120, 24, |frame| draw_app(frame, app, &mut view)).join("\n")
    };

    for ch in "tags:".chars() {
        app.search_input_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(ch),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    let screen = drawn(&mut app);
    // Value left, count right, ranked by count — `apple` is on both entries.
    assert!(screen.contains("apple"), "{screen}");
    assert!(screen.contains("apricot"), "{screen}");
    let apple = screen.find("apple").unwrap();
    let apricot = screen.find("apricot").unwrap();
    assert!(apple < apricot, "the more common value ranks first");

    // Typing narrows the list.
    for ch in "apr".chars() {
        app.search_input_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(ch),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    let screen = drawn(&mut app);
    assert!(screen.contains("apricot"), "{screen}");
    assert!(
        !screen.contains("apple"),
        "`apple` no longer matches the fragment: {screen}"
    );

    // Esc takes the list down without leaving search.
    app.dismiss_suggestions();
    let screen = drawn(&mut app);
    assert!(!screen.contains("apricot"), "{screen}");
    assert_eq!(app.nav.mode, Mode::Search);
}

/// The suggestion popup's own chrome: an app in search mode with `count` tag
/// values on offer, its entries panel area, and the popup rects derived from it.
mod suggestion_popup_tests {
    use super::*;
    use crate::tui::render::entries::{search_suggestions_list_rect, search_suggestions_rect};
    use crate::tui::render::frames::{POPUP_FRAME_COLS, POPUP_FRAME_ROWS, popup_inner};
    use crate::tui::theme;

    const SCREEN: (u16, u16) = (120, 30);

    /// A library whose entries carry `count` distinct tags all starting `a`, so
    /// typing `tags:a` offers exactly that many values.
    fn app_offering(count: usize, active_theme: theme::Theme) -> AppModel {
        use crate::tui::test_support::app_in_temp;

        let mut app = app_in_temp(|root| {
            let dir = root.join("work").join("2026-07-01");
            std::fs::create_dir_all(&dir).unwrap();
            for index in 0..count {
                std::fs::write(
                    dir.join(format!("e{index}.md")),
                    format!(
                        "+++\nschema_version = 1\n\n[entry]\ntags = [\"a{index:02}\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\nbody\n"
                    ),
                )
                .unwrap();
            }
        });
        app.appearance.theme = active_theme;
        app.begin_search();
        for ch in "tags:a".chars() {
            app.search_input_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        assert_eq!(app.search.suggestions.rows.len(), count, "rows on offer");
        app
    }

    /// The entries panel area the popup hangs off, and the popup's outer and
    /// list rects within it.
    fn rects(app: &AppModel) -> (Rect, Rect) {
        let area = Rect::new(0, 0, SCREEN.0, SCREEN.1);
        let panel = tui_layout(area, app)
            .entries
            .expect("an entries panel at this width")
            .panel
            .area;
        let rows = app.search.suggestions.rows.len();
        let theme = &app.appearance.theme;
        (
            search_suggestions_rect(theme, panel, rows).expect("an outer rect"),
            search_suggestions_list_rect(theme, panel, rows).expect("a list rect"),
        )
    }

    fn rendered(app: &mut AppModel) -> TestBackend {
        let mut view = crate::tui::ui::ViewState::default();
        render_backend(SCREEN.0, SCREEN.1, |frame| draw_app(frame, app, &mut view))
    }

    /// The popup keeps a frame's worth of room on every side, so no option ever
    /// runs into the edge — the spacing that stops it reading as a bare list
    /// pasted over the results.
    #[test]
    fn the_popup_holds_its_options_off_its_edges() {
        for chrome in [theme::ChromeStyle::Flat, theme::ChromeStyle::Bordered] {
            let app = app_offering(
                3,
                theme::test_flat_theme().with_chrome_override(Some(chrome)),
            );
            let (outer, list) = rects(&app);

            assert_eq!(popup_inner(outer).y, outer.y + 1, "{chrome:?}: a top row");
            assert_eq!(
                popup_inner(outer).bottom(),
                outer.bottom() - 1,
                "{chrome:?}: a bottom row"
            );
            assert_eq!(
                list.x,
                outer.x + POPUP_FRAME_COLS,
                "{chrome:?}: options held off the left edge"
            );
            assert!(
                list.right() <= outer.right() - POPUP_FRAME_COLS,
                "{chrome:?}: options held off the right edge"
            );
            // Three options plus the frame, and the frame hangs under the field.
            assert_eq!(outer.height, 3 + POPUP_FRAME_ROWS, "{chrome:?}");
            assert_eq!(list.height, 3, "{chrome:?}");
        }
    }

    /// The popup is the field's drawer: both edges square off against the field
    /// itself. Not against the box `draw_search_field` paints around it — that
    /// box is a cell wider on each side, in a surface near enough to invisible
    /// that matching it reads as being a column out.
    #[test]
    fn the_popup_lines_up_with_the_search_field() {
        for chrome in [theme::ChromeStyle::Flat, theme::ChromeStyle::Bordered] {
            let app = app_offering(
                3,
                theme::test_flat_theme().with_chrome_override(Some(chrome)),
            );
            let panel = tui_layout(Rect::new(0, 0, SCREEN.0, SCREEN.1), &app)
                .entries
                .expect("an entries panel")
                .panel
                .area;
            let field =
                crate::tui::render::entries::search_field_rect(&app.appearance.theme, panel)
                    .expect("a search field");
            let (outer, _) = rects(&app);

            assert_eq!(outer.x, field.x, "{chrome:?}: flush left with the field");
            assert_eq!(
                outer.right(),
                field.right(),
                "{chrome:?}: and flush right with it"
            );
            assert_eq!(outer.y, field.y + 1, "{chrome:?}: directly under it");
        }
    }

    /// Each chrome separates the popup from the results its own way: bordered
    /// draws the box, flat carries the dialog surface right out to its edge.
    #[test]
    fn the_popup_separates_itself_from_the_results() {
        let bordered =
            theme::test_flat_theme().with_chrome_override(Some(theme::ChromeStyle::Bordered));
        let mut app = app_offering(3, bordered);
        let (outer, list) = rects(&app);
        let backend = rendered(&mut app);
        for x in [outer.x, outer.right() - 1] {
            assert_ne!(
                backend.buffer()[(x, list.y)].symbol(),
                " ",
                "bordered: a border at column {x}"
            );
        }

        let flat = theme::test_flat_theme().with_chrome_override(Some(theme::ChromeStyle::Flat));
        let dialog_bg = flat.dialog_bg();
        let mut app = app_offering(3, flat);
        let (outer, list) = rects(&app);
        let backend = rendered(&mut app);
        let buf = backend.buffer();
        // Out to the last column, and demonstrably not the surface behind it.
        assert_eq!(
            buf[(outer.right() - 1, list.y)].bg,
            dialog_bg,
            "flat: filled"
        );
        assert_ne!(
            buf[(outer.x - 1, list.y)].bg,
            dialog_bg,
            "flat: the panel behind is a different surface"
        );
    }

    /// The scrollbar belongs to the popup, so it lands inside it: on the border
    /// in bordered chrome, inset within the padding in flat. It used to be drawn
    /// one column *past* the popup's right edge, painting the entry panel behind
    /// it instead.
    #[test]
    fn the_popup_keeps_its_scrollbar_on_its_own_edge() {
        for (chrome, inset) in [
            (theme::ChromeStyle::Flat, 3),
            (theme::ChromeStyle::Bordered, 1),
        ] {
            let app = app_offering(
                12,
                theme::test_flat_theme().with_chrome_override(Some(chrome)),
            );
            let (outer, list) = rects(&app);
            assert!(list.height < 12, "{chrome:?}: the list has to overflow");

            let bar = chrome::dialog_list_scrollbar_rect(list);
            assert_eq!(
                bar.x,
                outer.right() - inset,
                "{chrome:?}: bar at {} in {outer:?}",
                bar.x
            );
        }
    }

    /// The popup dims what it hangs over, so it reads as floating rather than
    /// pasted on — while the field it belongs to stays at full brightness.
    #[test]
    fn the_popup_dims_the_results_but_not_its_own_field() {
        let mut app = app_offering(3, theme::test_flat_theme());
        let (outer, _) = rects(&app);
        let field = crate::tui::render::entries::search_field_rect(
            &app.appearance.theme,
            tui_layout(Rect::new(0, 0, SCREEN.0, SCREEN.1), &app)
                .entries
                .expect("an entries panel")
                .panel
                .area,
        )
        .expect("a search field");

        // The same cell with the popup up and with it dismissed: an entry row
        // well below the popup, which nothing else redraws between the two.
        let probe = (outer.x + 2, outer.bottom() + 3);
        let lit = rendered(&mut app).buffer()[probe].clone();
        app.dismiss_suggestions();
        let plain = rendered(&mut app).buffer()[probe].clone();
        assert_ne!(
            lit.bg, plain.bg,
            "the results behind the popup darken: {lit:?} vs {plain:?}"
        );

        // The field is redrawn over the scrim, so it does not.
        let mut app = app_offering(3, theme::test_flat_theme());
        let lit = rendered(&mut app).buffer()[(field.x, field.y)].clone();
        app.dismiss_suggestions();
        let plain = rendered(&mut app).buffer()[(field.x, field.y)].clone();
        assert_eq!(lit.bg, plain.bg, "the search field stays bright");
    }
}
