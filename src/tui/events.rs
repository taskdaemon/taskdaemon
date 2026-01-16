//! TUI event handling

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, KeyEvent, MouseEvent};
use eyre::Result;

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
    rx: mpsc::Receiver<Event>,
    /// Event sender (kept for potential future use)
    #[allow(dead_code)]
    tx: mpsc::Sender<Event>,
}

impl EventHandler {
    /// Create a new event handler with the given tick rate
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let event_tx = tx.clone();

        // Spawn event polling thread
        thread::spawn(move || {
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

                        if event_tx.send(event).is_err() {
                            break;
                        }
                    }
                } else {
                    // Send tick event
                    if event_tx.send(Event::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        Self { rx, tx }
    }

    /// Get the next event (blocking)
    pub fn next(&self) -> Result<Event> {
        Ok(self.rx.recv()?)
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
