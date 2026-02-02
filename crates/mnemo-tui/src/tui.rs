//! Terminal user interface setup and management

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, stdout, Stdout};

/// Terminal UI wrapper
pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    /// Create a new TUI instance
    pub fn new() -> io::Result<Self> {
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// Enter the TUI mode (raw mode, alternate screen)
    pub fn enter(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        self.terminal.clear()?;
        Ok(())
    }

    /// Exit the TUI mode and restore terminal state
    pub fn exit(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(stdout(), LeaveAlternateScreen)?;
        Ok(())
    }

    /// Get a mutable reference to the terminal for drawing
    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Attempt to restore terminal state on drop
        let _ = self.exit();
    }
}
