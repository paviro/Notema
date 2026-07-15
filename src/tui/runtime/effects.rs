use crate::{
    AppResult,
    tui::{
        app::AppModel,
        events::{self, BackgroundAction, ControlFlow, DispatchOutcome, Effect, OpenTarget},
    },
};
use ratatui::{Terminal, backend::Backend};
use std::{collections::VecDeque, io};

/// Drain a frame's queued effects, executing each and folding any follow-up
/// dispatch back into `outcome`. External opens go through the real OS opener;
/// tests use [`execute_with_opener`] to inject a fake one.
pub(super) fn execute<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut AppModel,
    outcome: DispatchOutcome,
) -> AppResult<DispatchOutcome> {
    execute_with_opener(terminal, app, outcome, |target| match target {
        OpenTarget::Path(path) => open::that(path),
        OpenTarget::Uri(uri) => open::that(uri),
    })
}

/// The effect loop with the OS opener factored out so it can be driven under a
/// `TestBackend` without launching a real browser or file handler.
fn execute_with_opener<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut AppModel,
    mut outcome: DispatchOutcome,
    open: impl Fn(OpenTarget) -> io::Result<()>,
) -> AppResult<DispatchOutcome> {
    let mut pending = VecDeque::from(std::mem::take(&mut outcome.effects));
    while let Some(effect) = pending.pop_front() {
        match effect {
            Effect::Redraw => outcome.redraw = true,
            Effect::Geocode(request) => {
                app.geocode.request(request, crate::tui::geocode::resolve);
            }
            Effect::Environment(request) => {
                app.environment
                    .request(request, crate::tui::environment::resolve);
            }
            Effect::PrepareImages(request) => {
                app.image.runtime.warm(&request.assets, request.size);
            }
            Effect::Open {
                target,
                success_message,
            } => {
                let completion = match open(target) {
                    Ok(()) => BackgroundAction::ExternalOpenCompleted(success_message),
                    Err(error) => BackgroundAction::ExternalOpenFailed(error.to_string()),
                };
                let mut completed =
                    events::dispatch_action(terminal, app, events::Action::Background(completion))?;
                outcome.redraw |= completed.redraw;
                if completed.control == ControlFlow::Quit {
                    outcome.control = ControlFlow::Quit;
                }
                pending.extend(std::mem::take(&mut completed.effects));
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::ToastVariant;
    use crate::tui::test_support::app_with_journals;
    use ratatui::backend::TestBackend;

    fn terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    fn outcome_with(effects: Vec<Effect>) -> DispatchOutcome {
        DispatchOutcome {
            control: ControlFlow::Continue,
            redraw: false,
            effects,
        }
    }

    #[test]
    fn redraw_effect_marks_the_frame_dirty() {
        let mut app = app_with_journals(&["work"]);
        let mut term = terminal();

        let out = execute(&mut term, &mut app, outcome_with(vec![Effect::Redraw])).unwrap();

        assert!(out.redraw);
        assert!(out.effects.is_empty());
    }

    #[test]
    fn successful_open_shows_an_info_toast_and_repaints() {
        let mut app = app_with_journals(&["work"]);
        let mut term = terminal();

        let out = execute_with_opener(
            &mut term,
            &mut app,
            outcome_with(vec![Effect::Open {
                target: OpenTarget::Uri("https://example.com".to_string()),
                success_message: "Opened link".to_string(),
            }]),
            |_| Ok(()),
        )
        .unwrap();

        assert!(out.redraw);
        assert_eq!(out.control, ControlFlow::Continue);
        let toast = app.toasts.items().last().expect("info toast");
        assert_eq!(toast.variant, ToastVariant::Info);
        assert_eq!(toast.message, "Opened link");
    }

    #[test]
    fn failed_open_becomes_an_error_toast() {
        let mut app = app_with_journals(&["work"]);
        let mut term = terminal();

        let out = execute_with_opener(
            &mut term,
            &mut app,
            outcome_with(vec![Effect::Open {
                target: OpenTarget::Path("/does/not/exist".into()),
                success_message: "Opened link".to_string(),
            }]),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no handler")),
        )
        .unwrap();

        assert!(out.redraw);
        let toast = app.toasts.items().last().expect("error toast");
        assert_eq!(toast.variant, ToastVariant::Error);
        assert_eq!(toast.message, "Couldn't open link: no handler");
    }

    #[test]
    fn every_queued_effect_drains() {
        let mut app = app_with_journals(&["work"]);
        let mut term = terminal();

        let out = execute_with_opener(
            &mut term,
            &mut app,
            outcome_with(vec![
                Effect::Redraw,
                Effect::Open {
                    target: OpenTarget::Uri("https://example.com".to_string()),
                    success_message: "Opened link".to_string(),
                },
            ]),
            |_| Ok(()),
        )
        .unwrap();

        assert!(out.redraw);
        assert!(out.effects.is_empty());
        assert_eq!(app.toasts.items().last().unwrap().message, "Opened link");
    }
}
