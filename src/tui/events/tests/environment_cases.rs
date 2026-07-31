//! The fetching-environment modal: waiting on the editor's background fetch,
//! resuming the deferred save when it lands, and giving up on timeout.

use super::*;
use std::time::{Duration, Instant};

/// An app with the lone entry open in the editor (unchanged buffer), the
/// environment fetch pending under the modal — where every poll test starts.
fn app_waiting_on_fetch() -> AppModel {
    let mut app = app_with_entry();
    app.open_editor_for_selected().unwrap();
    app.editor.as_mut().unwrap().pending_environment = Some(1);
    app.overlay = Overlay::FetchingEnvironment(Instant::now());
    app
}

#[test]
fn fetching_environment_keeps_waiting_while_the_fetch_is_pending() {
    let mut app = app_waiting_on_fetch();

    assert!(!poll_fetching_environment(&mut app));

    assert!(matches!(app.overlay, Overlay::FetchingEnvironment(_)));
    assert_eq!(app.editor.as_ref().unwrap().pending_environment, Some(1));
}

#[test]
fn fetching_environment_saves_once_the_fetch_lands() {
    let mut app = app_waiting_on_fetch();
    app.editor.as_mut().unwrap().pending_environment = None;

    assert!(poll_fetching_environment(&mut app));

    // The deferred save re-ran: unchanged buffer, so the editor closed onto the
    // reader through the "No changes" path.
    assert!(matches!(app.overlay, Overlay::None));
    assert!(app.editor.is_none());
    assert_eq!(app.nav.focus, Focus::Reader);
}

#[test]
fn fetching_environment_gives_up_after_the_timeout_and_saves_bare() {
    let mut app = app_waiting_on_fetch();
    let expired = Instant::now().checked_sub(Duration::from_secs(11)).unwrap();
    app.overlay = Overlay::FetchingEnvironment(expired);

    assert!(poll_fetching_environment(&mut app));

    // The pending fetch was abandoned so the save could proceed without it.
    assert!(matches!(app.overlay, Overlay::None));
    assert!(app.editor.is_none());
}

#[test]
fn fetching_environment_ignores_a_closed_modal() {
    let mut app = app_with_entry();

    assert!(!poll_fetching_environment(&mut app));
}
