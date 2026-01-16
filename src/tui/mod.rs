//! Terminal User Interface for TaskDaemon
//!
//! Provides a real-time dashboard showing:
//! - Running loops with status
//! - Progress updates and metrics
//! - Resource utilization

mod app;
mod events;
mod views;

pub use app::{App, AppMode};
pub use events::{Event, EventHandler};
pub use views::render;

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use eyre::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

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

/// Run the TUI application
pub async fn run(mut terminal: Tui) -> Result<()> {
    let mut app = App::new();
    let event_handler = EventHandler::new(Duration::from_millis(100));

    loop {
        // Draw the UI
        terminal.draw(|frame| views::render(&mut app, frame))?;

        // Handle events
        match event_handler.next()? {
            Event::Tick => {
                app.tick();
            }
            Event::Key(key_event) => {
                if app.handle_key(key_event) {
                    break;
                }
            }
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
        }
    }

    Ok(())
}
