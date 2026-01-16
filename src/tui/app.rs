//! TUI application - event handling and state management
//!
//! The App struct owns the AppState and handles all keyboard events.
//! It does not do any rendering - that's delegated to the views module.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{AppState, ConfirmAction, ConfirmDialog, InteractionMode, LayoutMode, ResourceView, SortOrder};

/// TUI application
#[derive(Debug)]
pub struct App {
    /// Application state
    state: AppState,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Create a new application instance
    pub fn new() -> Self {
        Self { state: AppState::new() }
    }

    /// Get reference to state
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Get mutable reference to state
    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Handle a key event
    ///
    /// Returns true if the application should exit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Clear any transient error message on key press
        self.state.clear_error();

        // Handle based on interaction mode
        match &self.state.interaction_mode {
            InteractionMode::Normal => self.handle_normal_key(key),
            InteractionMode::Search(_) => self.handle_search_key(key),
            InteractionMode::Command(_) => self.handle_command_key(key),
            InteractionMode::Confirm(_) => self.handle_confirm_key(key),
            InteractionMode::Help => self.handle_help_key(key),
        }
    }

    /// Handle key in normal mode
    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            // === Quit ===
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                return true; // Force quit
            }
            (KeyCode::Char('q'), _) => {
                // Show confirmation if there are active loops
                if self.state.metrics.ralphs_active > 0 {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
                } else {
                    self.state.should_quit = true;
                }
            }

            // === Help ===
            (KeyCode::Char('?'), _) | (KeyCode::F(1), _) => {
                self.state.interaction_mode = InteractionMode::Help;
            }

            // === Mode switching ===
            (KeyCode::Char('/'), _) => {
                self.state.interaction_mode = InteractionMode::Search(String::new());
            }
            (KeyCode::Char(':'), _) => {
                self.state.interaction_mode = InteractionMode::Command(String::new());
            }

            // === Navigation ===
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                self.state.current_selection_mut().select_prev();
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                let max = self.state.current_items().len();
                self.state.current_selection_mut().select_next(max);
            }
            (KeyCode::Char('g'), _) => {
                self.state.current_selection_mut().select_first();
            }
            (KeyCode::Char('G'), _) => {
                let max = self.state.current_items().len();
                self.state.current_selection_mut().select_last(max);
            }

            // === Drill down / back ===
            (KeyCode::Enter, _) => {
                self.state.drill_down();
            }
            (KeyCode::Esc, _) => {
                // Clear filter first, then navigate back, then show quit confirm
                if self.state.filter.is_active() {
                    self.state.filter.clear();
                } else if !self.state.navigate_back() {
                    // No more history, ask to quit
                    if self.state.metrics.ralphs_active > 0 {
                        self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
                    } else {
                        self.state.should_quit = true;
                    }
                }
            }

            // === Layout modes ===
            (KeyCode::Char('d'), _) => {
                self.state.layout_mode = LayoutMode::Dashboard;
            }
            (KeyCode::Char(' '), _) => {
                self.state.layout_mode = self.state.layout_mode.next();
            }
            (KeyCode::Char('f'), _) => {
                self.state.layout_mode = LayoutMode::Focus;
            }

            // === Sort ===
            (KeyCode::Char('s'), _) => {
                self.state.sort_order = self.state.sort_order.next();
            }

            // === Refresh ===
            (KeyCode::Char('r'), _) => {
                // Force refresh by setting last_refresh to 0
                self.state.last_refresh = 0;
            }

            // === Loop actions ===
            (KeyCode::Char('p'), _) => {
                // Pause selected loop
                if let Some(item) = self.state.selected_item()
                    && item.status == "running"
                {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                        ConfirmAction::PauseLoop(item.id.clone()),
                        format!("Pause loop {}?", item.name),
                    ));
                }
            }
            (KeyCode::Char('x'), _) => {
                // Cancel selected loop
                if let Some(item) = self.state.selected_item()
                    && !["complete", "failed", "cancelled", "stopped"].contains(&item.status.as_str())
                {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                        ConfirmAction::CancelLoop(item.id.clone()),
                        format!("Cancel loop {}?", item.name),
                    ));
                }
            }
            (KeyCode::Char('R'), _) => {
                // Restart selected loop
                if let Some(item) = self.state.selected_item()
                    && ["failed", "stopped"].contains(&item.status.as_str())
                {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                        ConfirmAction::RestartLoop(item.id.clone()),
                        format!("Restart loop {}?", item.name),
                    ));
                }
            }

            // === Number keys for quick jump ===
            (KeyCode::Char(c), _) if c.is_ascii_digit() => {
                let index = c.to_digit(10).unwrap() as usize;
                if index > 0 {
                    let max = self.state.current_items().len();
                    if index <= max {
                        self.state.current_selection_mut().selected_index = index - 1;
                    }
                }
            }

            _ => {}
        }

        false
    }

    /// Handle key in search mode
    fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                // Apply the search filter
                if let InteractionMode::Search(text) = &self.state.interaction_mode {
                    self.state.filter.text = text.clone();
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Backspace => {
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.push(c);
                }
            }
            _ => {}
        }

        false
    }

    /// Handle key in command mode
    fn handle_command_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                // Execute the command
                if let InteractionMode::Command(cmd) = &self.state.interaction_mode {
                    self.execute_command(cmd.clone());
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Backspace => {
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.push(c);
                }
            }
            _ => {}
        }

        false
    }

    /// Handle key in confirm dialog
    fn handle_confirm_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                if let InteractionMode::Confirm(dialog) = &self.state.interaction_mode
                    && dialog.selected_button
                {
                    // User confirmed - execute the action
                    match &dialog.action {
                        ConfirmAction::Quit => {
                            self.state.should_quit = true;
                        }
                        ConfirmAction::CancelLoop(id) => {
                            // TODO: Send cancel command to StateManager
                            self.state.set_error(format!("Cancel {} - not yet implemented", id));
                        }
                        ConfirmAction::PauseLoop(id) => {
                            // TODO: Send pause command to StateManager
                            self.state.set_error(format!("Pause {} - not yet implemented", id));
                        }
                        ConfirmAction::RestartLoop(id) => {
                            // TODO: Send restart command to StateManager
                            self.state.set_error(format!("Restart {} - not yet implemented", id));
                        }
                        ConfirmAction::DeletePlan(id) => {
                            // TODO: Send delete command to StateManager
                            self.state.set_error(format!("Delete {} - not yet implemented", id));
                        }
                    }
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Toggle button selection
                if let InteractionMode::Confirm(dialog) = &mut self.state.interaction_mode {
                    if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
                        dialog.selected_button = true;
                    } else {
                        dialog.selected_button = !dialog.selected_button;
                    }
                }
            }
            _ => {}
        }

        false
    }

    /// Handle key in help mode
    fn handle_help_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                self.state.interaction_mode = InteractionMode::Normal;
            }
            _ => {}
        }

        false
    }

    /// Execute a command from command mode
    fn execute_command(&mut self, cmd: String) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }

        let command = parts[0];
        let args: Vec<&str> = parts[1..].to_vec();

        match command {
            // View switching
            "plans" | "specs" | "phases" | "ralphs" | "loops" | "metrics" | "costs" | "history" | "deps"
            | "dependencies" => {
                if let Some(view) = ResourceView::from_command(command) {
                    self.state.navigate_to(view);

                    // If there's an argument, use it as parent filter
                    if !args.is_empty()
                        && let Some(sel) = self.state.selection.get_mut(&view)
                    {
                        sel.parent_filter = Some(args[0].to_string());
                    }
                }
            }

            // Filtering
            "filter" => {
                if !args.is_empty() {
                    self.state.filter.text = args.join(" ");
                } else {
                    self.state.filter.clear();
                }
            }

            // Sorting
            "sort" => {
                if !args.is_empty() {
                    match args[0] {
                        "status" => self.state.sort_order = SortOrder::Status,
                        "name" => self.state.sort_order = SortOrder::Name,
                        "activity" => self.state.sort_order = SortOrder::Activity,
                        "priority" => self.state.sort_order = SortOrder::Priority,
                        _ => {
                            self.state.set_error(format!("Unknown sort field: {}", args[0]));
                        }
                    }
                }
            }

            // Loop actions
            "pause" => {
                if !args.is_empty() {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                        ConfirmAction::PauseLoop(args[0].to_string()),
                        format!("Pause loop {}?", args[0]),
                    ));
                }
            }
            "resume" => {
                if !args.is_empty() {
                    // TODO: Implement resume via StateManager
                    self.state
                        .set_error(format!("Resume {} - not yet implemented", args[0]));
                }
            }
            "cancel" => {
                if !args.is_empty() {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                        ConfirmAction::CancelLoop(args[0].to_string()),
                        format!("Cancel loop {}?", args[0]),
                    ));
                }
            }

            // Quit
            "quit" | "q" => {
                if self.state.metrics.ralphs_active > 0 {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
                } else {
                    self.state.should_quit = true;
                }
            }

            // Help
            "help" => {
                self.state.interaction_mode = InteractionMode::Help;
            }

            _ => {
                self.state.set_error(format!("Unknown command: {}", command));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let app = App::new();
        assert_eq!(app.state().current_view, ResourceView::Plans);
        assert!(matches!(app.state().interaction_mode, InteractionMode::Normal));
    }

    #[test]
    fn test_app_quit_key() {
        let mut app = App::new();

        // Ctrl+C always quits immediately
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.handle_key(key));
    }

    #[test]
    fn test_app_help_toggle() {
        let mut app = App::new();

        // Press ? to show help
        let key = KeyEvent::from(KeyCode::Char('?'));
        app.handle_key(key);
        assert!(matches!(app.state().interaction_mode, InteractionMode::Help));

        // Press ? again to hide help
        let key = KeyEvent::from(KeyCode::Char('?'));
        app.handle_key(key);
        assert!(matches!(app.state().interaction_mode, InteractionMode::Normal));
    }

    #[test]
    fn test_app_search_mode() {
        let mut app = App::new();

        // Enter search mode
        let key = KeyEvent::from(KeyCode::Char('/'));
        app.handle_key(key);
        assert!(matches!(app.state().interaction_mode, InteractionMode::Search(_)));

        // Type some text
        app.handle_key(KeyEvent::from(KeyCode::Char('t')));
        app.handle_key(KeyEvent::from(KeyCode::Char('e')));
        app.handle_key(KeyEvent::from(KeyCode::Char('s')));
        app.handle_key(KeyEvent::from(KeyCode::Char('t')));

        if let InteractionMode::Search(text) = &app.state().interaction_mode {
            assert_eq!(text, "test");
        }

        // Press Enter to apply
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(app.state().interaction_mode, InteractionMode::Normal));
        assert_eq!(app.state().filter.text, "test");
    }

    #[test]
    fn test_app_command_mode() {
        let mut app = App::new();

        // Enter command mode
        let key = KeyEvent::from(KeyCode::Char(':'));
        app.handle_key(key);
        assert!(matches!(app.state().interaction_mode, InteractionMode::Command(_)));

        // Type "specs"
        for c in "specs".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }

        // Press Enter to execute
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.state().current_view, ResourceView::Specs);
    }

    #[test]
    fn test_app_navigation() {
        let mut app = App::new();

        // Add some mock items
        app.state_mut().plans = vec![
            super::super::state::ResourceItem {
                id: "plan-1".to_string(),
                name: "Plan 1".to_string(),
                resource_type: "plan".to_string(),
                status: "running".to_string(),
                parent_id: None,
                iteration: None,
                progress: None,
                last_activity: None,
                needs_attention: false,
                attention_reason: None,
            },
            super::super::state::ResourceItem {
                id: "plan-2".to_string(),
                name: "Plan 2".to_string(),
                resource_type: "plan".to_string(),
                status: "complete".to_string(),
                parent_id: None,
                iteration: None,
                progress: None,
                last_activity: None,
                needs_attention: false,
                attention_reason: None,
            },
        ];

        // Initial selection is 0
        assert_eq!(app.state().current_selection().selected_index, 0);

        // Move down
        app.handle_key(KeyEvent::from(KeyCode::Char('j')));
        assert_eq!(app.state().current_selection().selected_index, 1);

        // Move up
        app.handle_key(KeyEvent::from(KeyCode::Char('k')));
        assert_eq!(app.state().current_selection().selected_index, 0);
    }

    #[test]
    fn test_app_layout_toggle() {
        let mut app = App::new();
        assert_eq!(app.state().layout_mode, LayoutMode::Dashboard);

        // Press space to cycle
        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert_eq!(app.state().layout_mode, LayoutMode::Split);

        // Press 'd' to go back to dashboard
        app.handle_key(KeyEvent::from(KeyCode::Char('d')));
        assert_eq!(app.state().layout_mode, LayoutMode::Dashboard);

        // Press 'f' for focus mode
        app.handle_key(KeyEvent::from(KeyCode::Char('f')));
        assert_eq!(app.state().layout_mode, LayoutMode::Focus);
    }

    #[test]
    fn test_execute_command_view_switch() {
        let mut app = App::new();

        app.execute_command("specs".to_string());
        assert_eq!(app.state().current_view, ResourceView::Specs);

        app.execute_command("ralphs".to_string());
        assert_eq!(app.state().current_view, ResourceView::Ralphs);

        app.execute_command("metrics".to_string());
        assert_eq!(app.state().current_view, ResourceView::Metrics);
    }

    #[test]
    fn test_execute_command_filter() {
        let mut app = App::new();

        app.execute_command("filter test query".to_string());
        assert_eq!(app.state().filter.text, "test query");

        app.execute_command("filter".to_string());
        assert!(app.state().filter.text.is_empty());
    }

    #[test]
    fn test_execute_command_sort() {
        let mut app = App::new();

        app.execute_command("sort name".to_string());
        assert_eq!(app.state().sort_order, SortOrder::Name);

        app.execute_command("sort priority".to_string());
        assert_eq!(app.state().sort_order, SortOrder::Priority);
    }

    #[test]
    fn test_execute_command_unknown() {
        let mut app = App::new();

        app.execute_command("unknowncommand".to_string());
        assert!(app.state().error_message.is_some());
        assert!(app.state().error_message.as_ref().unwrap().contains("Unknown command"));
    }
}
