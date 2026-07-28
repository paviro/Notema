//! Where toasts stack, how each variant is edged, and the dismissal countdown.

use super::*;

#[test]
fn toast_renders_top_right_with_variant_edges() {
    let mut app = flat_app(app_with_journals(&["alpha"]));
    app.toast(crate::tui::state::ToastVariant::Success, "Entry saved");

    let backend = render_app(app, 120, 30);
    let buffer = backend.buffer();

    // Width 44 with a right inset of 2 → columns 74..=117, starting at row 1;
    // a one-line message makes a 3-row box.
    let success = theme::test_flat_theme().success();
    for y in 1..=3u16 {
        for x in [74u16, 117u16] {
            assert_eq!(buffer[(x, y)].symbol(), "┃", "edge missing at ({x},{y})");
            assert_eq!(buffer[(x, y)].fg, success.fg.unwrap());
        }
    }
    let message_row: String = (75..117)
        .map(|x| buffer[(x as u16, 2u16)].symbol())
        .collect();
    assert!(
        message_row.contains("Entry saved"),
        "row was: {message_row}"
    );
    // The card sits on the element surface — one step off the panels it
    // floats over, so it separates without relying on the edge stripes.
    assert_eq!(
        buffer[(80u16, 2u16)].bg,
        theme::test_flat_theme().raised_bg()
    );
    // The top padding row stays blank.
    let top: String = (75..117)
        .map(|x| buffer[(x as u16, 1u16)].symbol())
        .collect();
    assert_eq!(top.trim(), "");
    // The bottom row carries the dismissal countdown line: a freshly-pushed
    // toast fills the inner span with an accent `─`, inset one column inside
    // each edge stripe so cols 75 and 116 stay blank.
    let countdown: String = (75..117)
        .map(|x| buffer[(x as u16, 3u16)].symbol())
        .collect();
    assert_eq!(countdown, format!(" {} ", "─".repeat(40)));
    assert_eq!(buffer[(76u16, 3u16)].fg, success.fg.unwrap());
}

#[test]
fn toasts_stack_with_a_blank_row_between() {
    let mut app = flat_app(app_with_journals(&["alpha"]));
    app.toast(crate::tui::state::ToastVariant::Info, "First");
    app.toast(crate::tui::state::ToastVariant::Error, "Second");

    let backend = render_app(app, 120, 30);
    let buffer = backend.buffer();

    // Oldest on top (rows 1..=3), a blank row, then the newest (rows 5..=7).
    let info = theme::test_flat_theme().info();
    let error = theme::test_flat_theme().error();
    assert_eq!(buffer[(74u16, 1u16)].fg, info.fg.unwrap());
    assert_ne!(buffer[(74u16, 4u16)].symbol(), "┃");
    assert_eq!(buffer[(74u16, 5u16)].symbol(), "┃");
    assert_eq!(buffer[(74u16, 5u16)].fg, error.fg.unwrap());
}

#[test]
fn expired_toast_shows_no_countdown_line() {
    let mut app = flat_app(app_with_journals(&["alpha"]));
    app.toasts
        .push_expired(crate::tui::state::ToastVariant::Info, "Gone");

    let backend = render_app(app, 120, 30);
    let buffer = backend.buffer();

    // Its remaining fraction is 0, so the bottom countdown row is blank.
    let countdown: String = (75..117)
        .map(|x| buffer[(x as u16, 3u16)].symbol())
        .collect();
    assert_eq!(countdown.trim(), "");
}

// Flat chrome has no top border for the box title to fold into, so the title
// takes its own inner row. These pin that the container is sized for it and
// the last line — the command / hint — stays inside the box.

#[test]
fn pending_notice_shows_the_enroll_command_in_flat_chrome() {
    let text = render_pending_notice_text_with_theme(
        &flat_theme(),
        "phone",
        &AccessNotice::NeedsEnroll { retired_key: false },
    );
    assert!(text.contains("Not authorized"));
    assert!(text.contains(crate::ENROLL_CMD));
}

#[test]
fn pending_notice_shows_the_approve_command_in_flat_chrome() {
    let text = render_pending_notice_text_with_theme(
        &flat_theme(),
        "phone",
        &AccessNotice::AwaitingApproval,
    );
    assert!(text.contains("Awaiting approval"));
    assert!(text.contains(&format!("{} phone", crate::APPROVE_CMD)));
}

#[test]
fn disable_notice_shows_the_enable_hint_in_flat_chrome() {
    let theme = flat_theme();
    let backend = render_backend(72, 20, |frame| draw_disable_notice(&theme, frame));
    let text: String = backend
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Encryption disabled"));
    assert!(text.contains("notema encryption enable"));
}

mod toast_bordered_tests {
    use super::*;

    #[test]
    fn toast_draws_a_variant_colored_border_box() {
        // The default (terminal/classic) theme is bordered chrome.
        let mut app = app_with_journals(&["alpha"]);
        app.toast(crate::tui::state::ToastVariant::Error, "Save failed");

        let backend = render_app(app, 120, 30);
        let buffer = backend.buffer();

        // The box grows one row to give the countdown a line above the bottom
        // border: ┌ row 1, message row 2, countdown row 3, └ row 4.
        assert_eq!(buffer[(74u16, 1u16)].symbol(), "┌");
        assert_eq!(buffer[(117u16, 1u16)].symbol(), "┐");
        assert_eq!(buffer[(74u16, 4u16)].symbol(), "└");
        assert_eq!(buffer[(117u16, 4u16)].symbol(), "┘");
        if let Some(fg) = crate::tui::theme::Theme::terminal_default().error().fg {
            assert_eq!(buffer[(74u16, 1u16)].fg, fg);
        }
        // Row 3 carries the countdown line, inset one column inside the border.
        let countdown: String = (75..117)
            .map(|x| buffer[(x as u16, 3u16)].symbol())
            .collect();
        assert_eq!(countdown, format!(" {} ", "─".repeat(40)));
        let message_row: String = (75..117)
            .map(|x| buffer[(x as u16, 2u16)].symbol())
            .collect();
        assert!(
            message_row.contains("Save failed"),
            "row was: {message_row}"
        );
    }
}
