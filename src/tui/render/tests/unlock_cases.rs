//! The unlock screen and the pending / disabled encryption notices.

use super::*;

fn render_unlock_text(input: &str, error: Option<&str>) -> String {
    let mut field = crate::tui::text_input::PassphraseInput::default();
    for ch in input.chars() {
        field.insert(ch);
    }
    render_to_text(60, 16, |frame| {
        draw_unlock(&theme::Theme::terminal_default(), frame, &field, error);
    })
}

#[test]
fn unlock_screen_masks_passphrase_and_draws_border() {
    let text = render_unlock_text("hunter2", None);
    // Bordered fullscreen chrome with the title and hint.
    assert!(text.contains("Unlock Notema"));
    assert!(text.contains("enter unlock"));
    assert!(text.contains("esc quit"));
    // The field sits in its own bordered box titled on the top-left.
    assert!(text.contains("Enter Password"));
    // The raw passphrase is never echoed; one '*' per character is.
    assert!(!text.contains("hunter2"));
    assert!(text.contains("*******"));
    // A standing hint sits below the field when there's no error.
    assert!(text.contains("Enter your passphrase to unlock"));
}

#[test]
fn unlock_screen_replaces_hint_with_error() {
    let text = render_unlock_text("", Some("Incorrect passphrase"));
    // The error takes the hint's place after a wrong passphrase.
    assert!(text.contains("Incorrect passphrase"));
    assert!(!text.contains("Enter your passphrase to unlock"));
}

fn render_unlock_rows(width: u16, height: u16, error: Option<&str>) -> Vec<String> {
    let input = crate::tui::text_input::PassphraseInput::default();
    render_to_rows(width, height, |frame| {
        draw_unlock(&theme::Theme::terminal_default(), frame, &input, error);
    })
    .into_iter()
    .map(|row| row.trim().to_string())
    .collect()
}

#[test]
fn unlock_status_wraps_on_a_narrow_terminal() {
    // Too narrow to fit the hint on one line: it must wrap across rows rather
    // than clip, so every word survives.
    let rows = render_unlock_rows(24, 20, None);
    let your_row = rows.iter().position(|r| r.contains("your"));
    let phrase_row = rows.iter().position(|r| r.contains("passphrase"));
    // Both hint words render in full (not truncated) on separate rows.
    assert!(your_row.is_some() && phrase_row.is_some());
    assert_ne!(your_row, phrase_row);
}

fn render_pending_notice_text(device_name: &str, notice: &AccessNotice) -> String {
    render_pending_notice_text_with_theme(&theme::Theme::terminal_default(), device_name, notice)
}

#[test]
fn pending_notice_wraps_in_the_journal_chrome_frame() {
    let text =
        render_pending_notice_text("phone", &AccessNotice::NeedsEnroll { retired_key: false });
    // Outer Notema chrome frame with its dismiss hint, plus the inner state box.
    assert!(text.contains("Notema"));
    assert!(text.contains("any key to exit"));
    assert!(text.contains("Not authorized"));
    assert!(text.contains("Device 'phone'"));
    assert!(text.contains(crate::ENROLL_CMD));
}

#[test]
fn pending_notice_only_mentions_a_retired_key_when_one_was_retired() {
    let retired =
        render_pending_notice_text("phone", &AccessNotice::NeedsEnroll { retired_key: true });
    assert!(retired.contains("old key has been retired"));

    // A never-enrolled device never had a key, so the line is omitted.
    let fresh = render_pending_notice_text("", &AccessNotice::NeedsEnroll { retired_key: false });
    assert!(!fresh.contains("old key has been retired"));
    // A keyless device reads as the sentence subject "This device", not a name.
    assert!(fresh.contains("This device"));
}

#[test]
fn pending_notice_awaiting_points_at_approval() {
    let text = render_pending_notice_text("phone", &AccessNotice::AwaitingApproval);
    assert!(text.contains("Awaiting approval"));
    assert!(text.contains(&format!("{} phone", crate::APPROVE_CMD)));
    assert!(!text.contains("old key has been retired"));
}

#[test]
fn disable_notice_renders_in_the_journal_chrome_frame() {
    let backend = TestBackend::new(72, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_disable_notice(&theme::Theme::terminal_default(), frame))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Notema"));
    assert!(text.contains("any key to continue"));
    assert!(text.contains("Encryption disabled"));
    assert!(text.contains("notema encryption enable"));
}
