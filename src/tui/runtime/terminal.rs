use crate::{AppResult, tui::runtime::CrosstermBackend};
use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use std::io::{self, Write};

pub(super) fn with_terminal(
    inner: impl FnOnce(&mut Terminal<CrosstermBackend<io::Stdout>>) -> AppResult<()>,
) -> AppResult<()> {
    enable_raw_mode()?;
    let mut terminal_guard = TerminalRestoreGuard::new();
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        SetCursorStyle::BlinkingBar
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = inner(&mut terminal);
    let restore_result = restore_terminal(terminal.backend_mut());
    if restore_result.is_ok() {
        terminal_guard.disarm();
    }

    match result {
        Ok(()) => restore_result,
        Err(error) => Err(error),
    }
}

struct TerminalRestoreGuard {
    active: bool,
}

impl TerminalRestoreGuard {
    fn new() -> Self {
        Self { active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = restore_terminal(&mut io::stdout());
        }
    }
}

fn restore_terminal(output: &mut impl Write) -> AppResult<()> {
    disable_raw_mode()?;
    execute!(
        output,
        DisableMouseCapture,
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape,
        Show
    )?;
    Ok(())
}
