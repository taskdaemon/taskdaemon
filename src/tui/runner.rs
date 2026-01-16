//! TUI Runner - main loop that owns terminal and polls StateManager
//!
//! The TuiRunner is responsible for:
//! - Initializing and restoring the terminal
//! - Polling StateManager for data updates (1s interval)
//! - Dispatching events to App for handling
//! - Rendering at ~30 FPS

use std::time::{Duration, Instant};

use eyre::Result;
use tracing::{debug, warn};

use crate::state::StateManager;

use super::Tui;
use super::app::App;
use super::events::{Event, EventHandler};
use super::state::{ResourceItem, ResourceView};
use super::views;

/// How often to refresh data from StateManager
const DATA_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// TUI Runner that manages the terminal and event loop
pub struct TuiRunner {
    /// Application state
    app: App,
    /// Terminal handle
    terminal: Tui,
    /// StateManager for data
    state_manager: Option<StateManager>,
    /// Event handler
    event_handler: EventHandler,
    /// Last data refresh time
    last_refresh: Instant,
}

impl TuiRunner {
    /// Create a new TuiRunner without StateManager (for testing/standalone mode)
    pub fn new(terminal: Tui) -> Self {
        Self {
            app: App::new(),
            terminal,
            state_manager: None,
            event_handler: EventHandler::new(Duration::from_millis(33)), // ~30 FPS
            last_refresh: Instant::now(),
        }
    }

    /// Create a new TuiRunner with StateManager connection
    pub fn with_state_manager(terminal: Tui, state_manager: StateManager) -> Self {
        Self {
            app: App::new(),
            terminal,
            state_manager: Some(state_manager),
            event_handler: EventHandler::new(Duration::from_millis(33)),
            last_refresh: Instant::now() - DATA_REFRESH_INTERVAL, // Force immediate refresh
        }
    }

    /// Run the TUI main loop
    pub async fn run(&mut self) -> Result<()> {
        // Fetch initial data if we have a state manager
        if self.state_manager.is_some() {
            self.refresh_data().await?;
        }

        loop {
            // Draw the UI
            self.terminal.draw(|frame| views::render(&mut self.app, frame))?;

            // Handle events
            match self.event_handler.next()? {
                Event::Tick => {
                    self.handle_tick().await?;
                }
                Event::Key(key_event) => {
                    if self.handle_key(key_event) {
                        break;
                    }
                }
                Event::Mouse(mouse_event) => {
                    self.handle_mouse(mouse_event);
                }
                Event::Resize(width, height) => {
                    self.handle_resize(width, height);
                }
            }

            // Check if we should quit
            if self.app.state().should_quit {
                break;
            }
        }

        Ok(())
    }

    /// Handle tick event - periodic updates
    async fn handle_tick(&mut self) -> Result<()> {
        self.app.state_mut().tick();

        // Refresh data if interval has elapsed
        if self.state_manager.is_some() && self.last_refresh.elapsed() >= DATA_REFRESH_INTERVAL {
            self.refresh_data().await?;
            self.last_refresh = Instant::now();
        }

        Ok(())
    }

    /// Handle key event
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.app.handle_key(key)
    }

    /// Handle mouse event
    fn handle_mouse(&mut self, _mouse: crossterm::event::MouseEvent) {
        // Mouse support is secondary to keyboard; implement basic click-to-select later
    }

    /// Handle terminal resize
    fn handle_resize(&mut self, _width: u16, _height: u16) {
        // Terminal handles resize automatically, but we might want to adjust state
        debug!("Terminal resized");
    }

    /// Refresh data from StateManager
    async fn refresh_data(&mut self) -> Result<()> {
        let state_manager = match &self.state_manager {
            Some(sm) => sm,
            None => return Ok(()),
        };

        // Sync plans
        match state_manager.list_plans(None).await {
            Ok(plans) => {
                let items: Vec<ResourceItem> = plans
                    .iter()
                    .map(|p| ResourceItem {
                        id: p.id.clone(),
                        name: p.title.clone(),
                        resource_type: "plan".to_string(),
                        status: p.status.to_string(),
                        parent_id: None,
                        iteration: None,
                        progress: None,
                        last_activity: None,
                        needs_attention: matches!(p.status, crate::domain::PlanStatus::Failed),
                        attention_reason: if p.status == crate::domain::PlanStatus::Failed {
                            Some("Plan failed".to_string())
                        } else {
                            None
                        },
                    })
                    .collect();

                let state = self.app.state_mut();
                state.metrics.plans_total = items.len();
                state.metrics.plans_active = items.iter().filter(|p| p.status == "in_progress").count();
                state.plans = items;
            }
            Err(e) => {
                warn!("Failed to fetch plans: {}", e);
                self.app.state_mut().set_error(format!("Failed to fetch plans: {}", e));
            }
        }

        // Sync specs
        let parent_filter = self
            .app
            .state()
            .selection
            .get(&ResourceView::Specs)
            .and_then(|s| s.parent_filter.clone());

        match state_manager.list_specs(parent_filter, None).await {
            Ok(specs) => {
                let items: Vec<ResourceItem> = specs
                    .iter()
                    .map(|s| {
                        let phase_progress = if !s.phases.is_empty() {
                            let complete = s.phases.iter().filter(|p| p.is_complete()).count();
                            Some(format!("{}/{}", complete, s.phases.len()))
                        } else {
                            None
                        };

                        ResourceItem {
                            id: s.id.clone(),
                            name: s.title.clone(),
                            resource_type: "spec".to_string(),
                            status: s.status.to_string(),
                            parent_id: Some(s.parent.clone()),
                            iteration: None,
                            progress: phase_progress,
                            last_activity: None,
                            needs_attention: matches!(
                                s.status,
                                crate::domain::SpecStatus::Blocked | crate::domain::SpecStatus::Failed
                            ),
                            attention_reason: match s.status {
                                crate::domain::SpecStatus::Blocked => Some("Blocked".to_string()),
                                crate::domain::SpecStatus::Failed => Some("Failed".to_string()),
                                _ => None,
                            },
                        }
                    })
                    .collect();

                let state = self.app.state_mut();
                state.metrics.specs_total = items.len();
                state.metrics.specs_running = items.iter().filter(|s| s.status == "running").count();
                state.specs = items;
            }
            Err(e) => {
                warn!("Failed to fetch specs: {}", e);
            }
        }

        // Sync phases (derived from specs)
        // Note: Phases are embedded in Specs, so we'd need to fetch full specs
        // For now, phases remain empty until we implement full spec fetching
        // In Phase 4, we'll add proper phase extraction from specs

        // Sync loop executions (ralphs)
        match state_manager.list_executions(None, None).await {
            Ok(executions) => {
                let items: Vec<ResourceItem> = executions
                    .iter()
                    .map(|e| ResourceItem {
                        id: e.id.clone(),
                        name: format!("{} ({})", e.loop_type, &e.id[..8.min(e.id.len())]),
                        resource_type: e.loop_type.clone(),
                        status: e.status.to_string(),
                        parent_id: e.parent.clone(),
                        iteration: Some(e.iteration),
                        progress: if e.progress.is_empty() {
                            None
                        } else {
                            Some(e.progress.lines().last().unwrap_or("").to_string())
                        },
                        last_activity: None,
                        needs_attention: matches!(
                            e.status,
                            crate::domain::LoopExecutionStatus::Blocked | crate::domain::LoopExecutionStatus::Failed
                        ),
                        attention_reason: e.last_error.clone(),
                    })
                    .collect();

                let state = self.app.state_mut();
                state.metrics.ralphs_active = items.iter().filter(|r| r.status == "running").count();
                state.metrics.ralphs_complete = items.iter().filter(|r| r.status == "complete").count();
                state.metrics.ralphs_failed = items.iter().filter(|r| r.status == "failed").count();
                state.metrics.total_iterations = items.iter().filter_map(|r| r.iteration).map(|i| i as u64).sum();
                state.ralphs = items;
            }
            Err(e) => {
                warn!("Failed to fetch executions: {}", e);
            }
        }

        // Update last refresh timestamp
        self.app.state_mut().last_refresh = taskstore::now_ms();

        // Clamp selections to valid ranges
        let state = self.app.state_mut();
        let plans_len = state.plans.len();
        let specs_len = state.specs.len();
        let phases_len = state.phases.len();
        let ralphs_len = state.ralphs.len();

        if let Some(sel) = state.selection.get_mut(&ResourceView::Plans) {
            sel.clamp(plans_len);
        }
        if let Some(sel) = state.selection.get_mut(&ResourceView::Specs) {
            sel.clamp(specs_len);
        }
        if let Some(sel) = state.selection.get_mut(&ResourceView::Phases) {
            sel.clamp(phases_len);
        }
        if let Some(sel) = state.selection.get_mut(&ResourceView::Ralphs) {
            sel.clamp(ralphs_len);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full integration tests require a terminal, which is difficult in CI.
    // These tests focus on the logic that can be tested without a terminal.

    #[test]
    fn test_data_refresh_interval() {
        assert_eq!(DATA_REFRESH_INTERVAL, Duration::from_secs(1));
    }
}
