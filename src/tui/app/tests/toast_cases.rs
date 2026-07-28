//! Toast lifetime: deadlines, expiry, the countdown redraw step, and the queue cap.

use super::*;

#[test]
fn toast_deadline_is_none_without_toasts() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let app = new_app(config);

    assert!(app.toast_deadline().is_none());
}

#[test]
fn toast_deadline_is_some_with_an_active_toast() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);

    app.toast(ToastVariant::Success, "Saved");

    assert!(app.toast_deadline().is_some());
}

#[test]
fn expire_toasts_drops_only_expired_ones_and_reports_once() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);
    app.toasts.push_expired(ToastVariant::Info, "Old");
    app.toast(ToastVariant::Success, "Fresh");

    assert!(app.expire_toasts());
    let messages: Vec<&str> = app
        .toasts
        .items()
        .iter()
        .map(|toast| toast.message.as_str())
        .collect();
    assert_eq!(messages, ["Fresh"]);
    assert!(!app.expire_toasts());
}

#[test]
fn next_countdown_step_is_at_most_one_column_of_lifetime() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);
    app.toast(ToastVariant::Success, "Saved");

    // A fresh toast over 40 columns steps roughly every lifetime/40; the wake is
    // scheduled no later than one such column so the shrink never skips a step.
    let step = app.toasts.next_countdown_step(40).unwrap();
    assert!(step <= std::time::Duration::from_millis(5000 / 40));

    // With no columns to draw (terminal too narrow) there is nothing to animate.
    assert!(app.toasts.next_countdown_step(0).is_none());
}

#[test]
fn long_messages_stay_up_longer_than_short_ones() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);

    // A short confirmation sits at the 5s floor.
    app.toast(ToastVariant::Success, "Saved");
    let short = app.toast_deadline().unwrap();
    assert!(short <= std::time::Duration::from_secs(5));

    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);
    // A long error lingers, capped at 10s.
    app.toast(ToastVariant::Error, "e".repeat(200));
    let long = app.toast_deadline().unwrap();
    assert!(long > std::time::Duration::from_secs(5));
    assert!(long <= std::time::Duration::from_secs(10));
}

#[test]
fn next_countdown_step_is_none_for_an_expired_toast() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);
    app.toasts.push_expired(ToastVariant::Info, "Old");

    // Its line is already empty, so there is no further column to schedule.
    assert!(app.toasts.next_countdown_step(40).is_none());
}

#[test]
fn toast_queue_caps_at_the_four_newest() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);

    for n in 0..6 {
        app.toast(ToastVariant::Info, format!("toast {n}"));
    }

    let messages: Vec<&str> = app
        .toasts
        .items()
        .iter()
        .map(|toast| toast.message.as_str())
        .collect();
    assert_eq!(messages, ["toast 2", "toast 3", "toast 4", "toast 5"]);
}
