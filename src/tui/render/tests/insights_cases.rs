//! The insights panel: its tab strip, and the overview, feelings, drivers and
//! writing bodies.

use super::*;

#[test]
fn insights_panel_shows_all_tabs_in_its_border() {
    let mut app = app_with_entry();
    focus_insights(&mut app, InsightsTab::Overview);
    // Wide enough that the strip uses full titles rather than short/initials.
    let text = render_text(app, 170, 20);

    for title in ["Overview", "Writing", "Feelings", "Drivers"] {
        assert!(text.contains(title), "tab bar missing {title}: {text}");
    }
}

#[test]
fn insights_overview_tab_shows_journal_summary() {
    let mut app = app_with_entry();
    focus_insights(&mut app, InsightsTab::Overview);
    let text = render_text(app, 140, 20);

    // The paired cards plus the totals in the title box.
    assert!(text.contains("Lifts you"));
    assert!(text.contains("Drains you"));
    assert!(text.contains("Happiest day"));
    assert!(text.contains("Active days"));
    assert!(text.contains("entry") || text.contains("entries"));
}

#[test]
fn insights_switching_tab_changes_the_body() {
    let mut app = app_with_entry();
    focus_insights(&mut app, InsightsTab::Feelings);

    let text = render_text(app, 140, 20);

    // The lone fixture entry has no mood and no feelings, so the merged Feelings tab
    // shows its empty state rather than the Overview cards.
    assert!(text.contains("No mood or feelings logged yet"));
    assert!(!text.contains("Days"));
}

#[test]
fn insights_feelings_tab_renders_frequency_bar() {
    let mut app = app_with_metadata_entry();
    focus_insights(&mut app, InsightsTab::Feelings);

    // Tall enough that the feelings table still fits below Balance + the breakdowns.
    let text = render_text(app, 140, 26);

    assert!(text.contains("calm"));
    assert!(text.contains('▓'), "expected a bar glyph: {text}");
}

/// A journal whose entries make `alex` a clear mood lift and `rain` a clear
/// drain, each appearing enough times (≥3) to clear the Drivers noise guard.
fn app_with_drivers() -> AppModel {
    let dir = tempdir().unwrap();
    let base = dir.path().join("work");
    let specs = [
        (5, "people = [\"alex\"]"),
        (5, "people = [\"alex\"]"),
        (5, "people = [\"alex\"]"),
        (-5, "tags = [\"rain\"]"),
        (-5, "tags = [\"rain\"]"),
        (-5, "tags = [\"rain\"]"),
    ];
    for (index, (mood, meta)) in specs.iter().enumerate() {
        let day = index + 1;
        let entry_dir = base.join(format!("2026-07-{day:02}"));
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(
            entry_dir.join("a.md"),
            format!(
                "+++\nschema_version = 1\n\n[entry]\n{meta}\nmood = {mood}\n\n[time]\ncreated_at = \"2026-07-{day:02}T10:00:00+02:00\"\n+++\n\n# E\nBody\n"
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

#[test]
fn insights_drivers_tab_ranks_lifts_and_drains() {
    let mut app = app_with_drivers();
    focus_insights(&mut app, InsightsTab::Drivers);

    let text = render_text(app, 140, 20);

    // People, activities, and tags are merged into one ranking.
    assert!(text.contains("alex"), "lifting person missing: {text}");
    assert!(text.contains("rain"), "draining tag missing: {text}");
}

#[test]
fn insights_drivers_tab_renders_headed_table_with_mood_bar() {
    let mut app = app_with_drivers();
    focus_insights(&mut app, InsightsTab::Drivers);
    // Expanded to full screen — the "bigger screen" case with room for the bar.
    app.nav.insights_fullscreen = true;

    let text = render_text(app, 140, 20);

    assert!(text.contains("Count"), "table header missing: {text}");
    assert!(
        text.contains("Drains / lifts"),
        "bar column missing: {text}"
    );
    assert!(text.contains('│'), "bar centre marker missing: {text}");
}

/// An app whose sole entry lists `count` people (`p00`..), so the People tab has a
/// list long enough to scroll.
/// A journal where people `p00`..`p{count-1}` each ride high moods across three
/// entries (clearing the ≥3 noise guard), plus baseline low-mood entries — so the
/// Drivers ranking is a list long enough to scroll.
fn app_with_many_drivers(count: usize) -> AppModel {
    let dir = tempdir().unwrap();
    let base = dir.path().join("work");
    let people: Vec<String> = (0..count).map(|i| format!("\"p{i:02}\"")).collect();
    let people = people.join(", ");
    // Three high-mood entries listing everyone, then two low-mood baselines.
    let specs = [
        (5, format!("people = [{people}]")),
        (5, format!("people = [{people}]")),
        (5, format!("people = [{people}]")),
        (-3, String::new()),
        (-3, String::new()),
    ];
    for (index, (mood, meta)) in specs.iter().enumerate() {
        let day = index + 1;
        let entry_dir = base.join(format!("2026-07-{day:02}"));
        fs::create_dir_all(&entry_dir).unwrap();
        let meta = if meta.is_empty() {
            String::new()
        } else {
            format!("{meta}\n")
        };
        fs::write(
            entry_dir.join("a.md"),
            format!(
                "+++\nschema_version = 1\n\n[entry]\n{meta}mood = {mood}\n\n[time]\ncreated_at = \"2026-07-{day:02}T10:00:00+02:00\"\n+++\n\n# A\nBody\n"
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

#[test]
fn insights_list_scrolls_to_reveal_later_rows() {
    let focused_drivers = |scroll: u16| {
        let mut app = app_with_many_drivers(30);
        focus_insights(&mut app, InsightsTab::Drivers);
        app.nav.insights_fullscreen = true;
        app.nav.scroll.insights = scroll;
        render_text(app, 120, 12)
    };

    // A short panel can't show every driver: the first is visible, a late one isn't.
    let top = focused_drivers(0);
    assert!(top.contains("p00"), "first row should be visible: {top}");
    assert!(!top.contains("p29"), "last row should be off-screen: {top}");

    // Jumping to the end (like the End key) reveals the last row and drops the first;
    // the render clamps the saturated offset to the final page.
    let bottom = focused_drivers(u16::MAX);
    assert!(
        bottom.contains("p29"),
        "last row should scroll into view: {bottom}"
    );
    assert!(
        !bottom.contains("p00"),
        "first row should scroll away: {bottom}"
    );
}

#[test]
fn insights_feelings_tab_shows_balance_and_feeling_table() {
    let mut app = app_with_metadata_entry();
    focus_insights(&mut app, InsightsTab::Feelings);

    let text = render_text(app, 140, 30);

    assert!(
        text.contains("Balance"),
        "feelings tab missing balance: {text}"
    );
    assert!(
        text.contains("calm"),
        "feelings table missing the feeling: {text}"
    );
}

#[test]
fn insights_writing_tab_renders_habit_sections() {
    let mut app = app_with_metadata_entry();
    focus_insights(&mut app, InsightsTab::Writing);
    // Wide + full screen so the weekday/hour charts sit side by side.
    app.nav.insights_fullscreen = true;

    let text = render_text(app, 140, 20);

    assert!(
        text.contains("Streak"),
        "writing tab missing streak: {text}"
    );
    assert!(
        text.contains("By weekday") && text.contains("By hour"),
        "writing tab missing side-by-side histograms: {text}"
    );
}

#[test]
fn insights_feelings_tab_renders_mood_breakdowns() {
    let mut app = app_with_metadata_entry();
    focus_insights(&mut app, InsightsTab::Feelings);
    // Wide + full screen so the three breakdown charts sit side by side.
    app.nav.insights_fullscreen = true;

    let text = render_text(app, 140, 24);

    // The merged tab carries the signed mood breakdowns below Balance...
    assert!(
        text.contains("By year") && text.contains("By weekday") && text.contains("By month"),
        "feelings tab missing the mood breakdown charts: {text}"
    );
    // ...and no separate "Mood over time" series.
    assert!(
        !text.contains("Mood over time"),
        "mood-over-time series should not render: {text}"
    );
}

#[test]
fn insights_tab_hit_test_maps_border_columns_to_tabs() {
    // Inner width 47 fits all four full labels: " Overview · Writing · Mood /
    // Feelings · Drivers", the title starting one past the corner at 75.
    let area = Rect::new(74, 0, 49, 19);
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 78, 0),
        Some(InsightsTab::Overview) // 76..84
    );
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 90, 0),
        Some(InsightsTab::Writing)
    ); // 87..94
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 100, 0),
        Some(InsightsTab::Feelings) // 97..112
    );
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 118, 0),
        Some(InsightsTab::Drivers)
    ); // 115..122
    // The corner, the gaps, and other rows are not tabs.
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 74, 0),
        None
    );
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 85, 0),
        None
    ); // " · " between Overview and Writing
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 96, 0),
        None
    ); // " · " between Writing and Mood / Feelings
    assert_eq!(
        insights_tab_at(&theme::Theme::terminal_default(), area, 78, 1),
        None
    );
}

#[test]
fn insights_active_tab_inverts_only_when_panel_is_focused() {
    // Focused: the active tab in the border uses the reversed style.
    let mut focused = app_with_entry();
    focus_insights(&mut focused, InsightsTab::Overview);
    let backend = render_app(focused, 140, 20);
    assert!(
        insights_border_has_reversed_text(&backend, 140),
        "focused panel should invert its active tab"
    );

    // Unfocused (Journals): the active tab is bold, not reversed. A focused
    // Journals panel reverses its own title, so the check is scoped to the insights
    // column to ignore that.
    let mut unfocused = app_with_entry();
    unfocused.nav.selected_entry_index = None;
    unfocused.nav.focus = Focus::Journals;
    let backend = render_app(unfocused, 140, 20);
    assert!(
        !insights_border_has_reversed_text(&backend, 140),
        "unfocused panel must not invert its active tab"
    );
}

/// Whether any non-blank cell in the insights panel's top border row is reversed —
/// the mark of the focused active tab. Scoped to the right-hand insights column
/// (past the journal + entry columns) so a focused Journals title doesn't count.
fn insights_border_has_reversed_text(backend: &TestBackend, width: u16) -> bool {
    const STATS_COLUMN_X: usize = 74;
    backend
        .buffer()
        .content()
        .iter()
        .take(width as usize)
        .skip(STATS_COLUMN_X)
        .any(|cell| cell.symbol() != " " && cell.modifier.contains(Modifier::REVERSED))
}
