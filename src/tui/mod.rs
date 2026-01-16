//! Terminal User Interface for TaskDaemon
//!
//! Provides a k9s-style real-time dashboard showing:
//! - Plans, Specs, Phases, and Ralphs (loop executions)
//! - Navigation with vim-style keybindings
//! - Command mode for quick actions
//! - TaskStore analytics views

mod app;
mod events;
mod runner;
pub mod state;
mod views;

pub use app::App;
pub use events::{Event, EventHandler};
pub use runner::TuiRunner;
pub use state::{AppState, InteractionMode, LayoutMode, ResourceView};

use std::io::{self, Stdout};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use eyre::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::state::StateManager;

/// Terminal type alias
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Initialize the terminal for TUI mode
pub fn init() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to normal mode
pub fn restore() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

/// Run the TUI application (standalone mode, no StateManager)
pub async fn run(terminal: Tui) -> Result<()> {
    let mut runner = TuiRunner::new(terminal);
    runner.run().await
}

/// Run the TUI with StateManager connection for live data
pub async fn run_with_state(state_manager: StateManager) -> Result<()> {
    let terminal = init()?;

    // Ensure terminal is restored even on panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut runner = TuiRunner::with_state_manager(terminal, state_manager);
            runner.run().await
        })
    }));

    restore()?;

    match result {
        Ok(inner_result) => inner_result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Run the TUI in a way that can be used from both sync and async contexts
pub fn run_blocking_with_state(state_manager: StateManager) -> Result<()> {
    let terminal = init()?;

    let result = {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let mut runner = TuiRunner::with_state_manager(terminal, state_manager);
            runner.run().await
        })
    };

    restore()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify that all public types are accessible
        let _: fn() -> App = App::new;
        let _: fn() -> AppState = AppState::new;
    }
}
