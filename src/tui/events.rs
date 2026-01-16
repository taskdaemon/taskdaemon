//! TUI event handling
//!
//! Async-compatible event handling for the TUI using tokio channels.

use std::time::Duration;

use crossterm::event::{self, KeyEvent, MouseEvent};
use eyre::Result;
use tokio::sync::mpsc;

/// Terminal events
#[derive(Debug)]
pub enum Event {
    /// Key press
    Key(KeyEvent),
    /// Mouse event
    Mouse(MouseEvent),
    /// Terminal resize
    Resize(u16, u16),
    /// Tick (periodic refresh)
    Tick,
}

/// Event handler for the TUI
pub struct EventHandler {
    /// Event receiver
    rx: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    /// Create a new event handler with the given tick rate
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn event polling task in a blocking thread
        std::thread::spawn(move || {
            loop {
                // Poll for events with timeout
                if event::poll(tick_rate).unwrap_or(false) {
                    if let Ok(evt) = event::read() {
                        let event = match evt {
                            event::Event::Key(key) => Event::Key(key),
                            event::Event::Mouse(mouse) => Event::Mouse(mouse),
                            event::Event::Resize(w, h) => Event::Resize(w, h),
                            _ => continue,
                        };

                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                } else {
                    // Send tick event
                    if tx.send(Event::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        Self { rx }
    }

    /// Get the next event (async)
    pub async fn next(&mut self) -> Result<Event> {
        self.rx.recv().await.ok_or_else(|| eyre::eyre!("Event channel closed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_handler_creation() {
        let _handler = EventHandler::new(Duration::from_millis(100));
        // Handler should be created without panic
    }
}
