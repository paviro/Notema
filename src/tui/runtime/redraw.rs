use std::io;

use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    AppResult,
    tui::{
        app::AppModel,
        events::{self, Action},
        render,
        ui::{RenderContext, ViewState},
    },
};

pub(super) fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppModel,
    view: &mut ViewState,
) -> AppResult<()> {
    let active_theme = app.appearance.theme.clone();
    let mut context = RenderContext::new(&active_theme, view);
    terminal.draw(|frame| render::draw(frame, app, &mut context))?;
    events::dispatch_action(
        terminal,
        app,
        Action::ViewRendered {
            reader_scroll: (view.reader.line_count > 0).then_some(view.reader.scroll),
            insights_scroll: (view.insights.total > 0).then_some(view.insights.scroll),
            journal_offset: view.journal_offset,
            entry_offset: view.entry_offset,
        },
    )?;
    Ok(())
}
