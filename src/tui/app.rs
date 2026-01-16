//! TUI application state

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Application mode (which view is active)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    /// Dashboard showing all loops
    #[default]
    Dashboard,
    /// Detailed view of a single loop
    LoopDetail,
    /// Metrics view
    Metrics,
    /// Help overlay
    Help,
}

/// Loop display state
#[derive(Debug, Clone)]
pub struct LoopDisplay {
    pub exec_id: String,
    pub loop_type: String,
    pub status: String,
    pub iteration: u32,
    pub progress: String,
    pub started_at: String,
}

impl LoopDisplay {
    /// Create a mock loop for testing
    pub fn mock(id: usize) -> Self {
        Self {
            exec_id: format!("loop-{:03}", id),
            loop_type: ["phase", "spec", "plan", "ralph"][id % 4].to_string(),
            status: ["running", "pending", "complete", "failed"][id % 4].to_string(),
            iteration: (id * 3) as u32,
            progress: format!("Working on item {}...", id),
            started_at: "2m ago".to_string(),
        }
    }
}

/// TUI application state
#[derive(Debug, Default)]
pub struct App {
    /// Current mode
    pub mode: AppMode,

    /// Selected loop index
    pub selected_loop: usize,

    /// Scroll offset for loop list
    pub scroll_offset: usize,

    /// Loop data (to be populated from state)
    pub loops: Vec<LoopDisplay>,

    /// Tick counter (for animations/refreshes)
    tick_count: u64,

    /// Global metrics summary
    pub metrics: MetricsSummary,
}

/// Metrics summary for display
#[derive(Debug, Default, Clone)]
pub struct MetricsSummary {
    pub active_loops: usize,
    pub completed_loops: usize,
    pub failed_loops: usize,
    pub total_iterations: u64,
    pub total_api_calls: u64,
}

impl App {
    /// Create a new application instance
    pub fn new() -> Self {
        // Initialize with mock data for now
        let loops: Vec<_> = (0..10).map(LoopDisplay::mock).collect();

        Self {
            mode: AppMode::Dashboard,
            selected_loop: 0,
            scroll_offset: 0,
            loops,
            tick_count: 0,
            metrics: MetricsSummary {
                active_loops: 3,
                completed_loops: 15,
                failed_loops: 2,
                total_iterations: 127,
                total_api_calls: 453,
            },
        }
    }

    /// Handle a tick event (periodic refresh)
    pub fn tick(&mut self) {
        self.tick_count += 1;
        // TODO: Refresh data from state manager
    }

    /// Handle a key event
    ///
    /// Returns true if the application should exit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Global keybindings
        match (key.code, key.modifiers) {
            // Quit
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                return true;
            }
            // Help toggle
            (KeyCode::Char('?'), _) | (KeyCode::F(1), _) => {
                self.mode = if self.mode == AppMode::Help { AppMode::Dashboard } else { AppMode::Help };
            }
            // Back to dashboard
            (KeyCode::Esc, _) => {
                if self.mode != AppMode::Dashboard {
                    self.mode = AppMode::Dashboard;
                } else {
                    return true;
                }
            }
            _ => {}
        }

        // Mode-specific keybindings
        match self.mode {
            AppMode::Dashboard => self.handle_dashboard_key(key),
            AppMode::LoopDetail => self.handle_detail_key(key),
            AppMode::Metrics => self.handle_metrics_key(key),
            AppMode::Help => {} // Help only responds to global keys
        }

        false
    }

    /// Handle dashboard-specific keys
    fn handle_dashboard_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_loop > 0 {
                    self.selected_loop -= 1;
                    // Adjust scroll if needed
                    if self.selected_loop < self.scroll_offset {
                        self.scroll_offset = self.selected_loop;
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_loop + 1 < self.loops.len() {
                    self.selected_loop += 1;
                }
            }
            KeyCode::Enter => {
                if !self.loops.is_empty() {
                    self.mode = AppMode::LoopDetail;
                }
            }
            KeyCode::Char('m') => {
                self.mode = AppMode::Metrics;
            }
            KeyCode::Char('r') => {
                // Refresh (TODO: trigger data refresh)
            }
            _ => {}
        }
    }

    /// Handle detail view keys
    fn handle_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if self.selected_loop > 0 {
                    self.selected_loop -= 1;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.selected_loop + 1 < self.loops.len() {
                    self.selected_loop += 1;
                }
            }
            _ => {}
        }
    }

    /// Handle metrics view keys
    fn handle_metrics_key(&mut self, _key: KeyEvent) {
        // Metrics view is mostly read-only
    }

    /// Get the currently selected loop
    pub fn selected_loop_data(&self) -> Option<&LoopDisplay> {
        self.loops.get(self.selected_loop)
    }

    /// Get the tick count
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let app = App::new();
        assert_eq!(app.mode, AppMode::Dashboard);
        assert!(!app.loops.is_empty());
    }

    #[test]
    fn test_app_navigation() {
        let mut app = App::new();

        // Down navigation
        app.handle_dashboard_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(app.selected_loop, 1);

        // Up navigation
        app.handle_dashboard_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(app.selected_loop, 0);
    }

    #[test]
    fn test_app_mode_switch() {
        let mut app = App::new();

        // Enter detail mode
        let key = KeyEvent::from(KeyCode::Enter);
        app.handle_key(key);
        assert_eq!(app.mode, AppMode::LoopDetail);

        // Back to dashboard
        let key = KeyEvent::from(KeyCode::Esc);
        app.handle_key(key);
        assert_eq!(app.mode, AppMode::Dashboard);
    }

    #[test]
    fn test_app_quit() {
        let mut app = App::new();

        // 'q' should quit
        let key = KeyEvent::from(KeyCode::Char('q'));
        assert!(app.handle_key(key));
    }
}
