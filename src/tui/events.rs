//! TUI event handling
//!
//! Async-compatible event handling for the TUI using tokio channels.

use std::time::Duration;

use crossterm::event::{self, KeyEvent, MouseEvent};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::debug;

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
        debug!(?tick_rate, "EventHandler::new: called");
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn event polling task in a blocking thread
        std::thread::spawn(move || {
            debug!("EventHandler::new: event polling thread started");
            loop {
                // Poll for events with timeout
                if event::poll(tick_rate).unwrap_or(false) {
                    if let Ok(evt) = event::read() {
                        let event = match evt {
                            event::Event::Key(key) => {
                                debug!(?key, "EventHandler: key event received");
                                Event::Key(key)
                            }
                            event::Event::Mouse(mouse) => {
                                debug!(?mouse, "EventHandler: mouse event received");
                                Event::Mouse(mouse)
                            }
                            event::Event::Resize(w, h) => {
                                debug!(w, h, "EventHandler: resize event received");
                                Event::Resize(w, h)
                            }
                            _ => {
                                debug!("EventHandler: other event, skipping");
                                continue;
                            }
                        };

                        if tx.send(event).is_err() {
                            debug!("EventHandler: channel closed, exiting loop");
                            break;
                        }
                    }
                } else {
                    // Send tick event
                    if tx.send(Event::Tick).is_err() {
                        debug!("EventHandler: channel closed on tick, exiting loop");
                        break;
                    }
                }
            }
            debug!("EventHandler: event polling thread exiting");
        });

        debug!("EventHandler::new: returning handler");
        Self { rx }
    }

    /// Get the next event (async)
    pub async fn next(&mut self) -> Result<Event> {
        debug!("EventHandler::next: called");
        let event = self.rx.recv().await.ok_or_else(|| eyre::eyre!("Event channel closed"));
        if let Ok(ref e) = event {
            debug!(?e, "EventHandler::next: received event");
        } else {
            debug!("EventHandler::next: channel closed");
        }
        event
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
