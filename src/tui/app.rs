//! TUI application - event handling and state management
//!
//! The App struct owns the AppState and handles all keyboard events.
//! It does not do any rendering - that's delegated to the views module.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{
    AppState, ConfirmAction, ConfirmDialog, InteractionMode, PendingAction, ReplMode, TOP_LEVEL_VIEWS, View,
    top_level_view_index,
};

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
            InteractionMode::Filter(_) => self.handle_filter_key(key),
            InteractionMode::Command(_) => self.handle_command_key(key),
            InteractionMode::TaskInput(_) => self.handle_task_input_key(key),
            InteractionMode::ReplInput => self.handle_repl_input_key(key),
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
                if self.state.executions_active > 0 {
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
                self.state.interaction_mode = InteractionMode::Filter(String::new());
            }
            (KeyCode::Char(':'), _) => {
                self.state.interaction_mode = InteractionMode::Command(String::new());
            }

            // === Top-level view navigation (left/right arrows) ===
            (KeyCode::Left, _) => {
                self.navigate_prev_top_level_view();
            }
            (KeyCode::Right, _) => {
                self.navigate_next_top_level_view();
            }

            // === Navigation (only in list views) ===
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                if let Some(sel) = self.state.current_selection_mut() {
                    sel.select_prev();
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                let max = self.state.current_item_count();
                if let Some(sel) = self.state.current_selection_mut() {
                    sel.select_next(max);
                }
            }
            (KeyCode::Char('g'), _) => {
                if let Some(sel) = self.state.current_selection_mut() {
                    sel.select_first();
                }
            }
            (KeyCode::Char('G'), _) => {
                let max = self.state.current_item_count();
                if let Some(sel) = self.state.current_selection_mut() {
                    sel.select_last(max);
                }
            }

            // === Drill down / back ===
            (KeyCode::Enter, _) => {
                self.handle_drill_down();
            }
            (KeyCode::Esc, _) => {
                self.handle_escape();
            }

            // === Actions ===
            (KeyCode::Char('l'), _) => {
                // View logs for selected item
                if let Some(id) = self.state.selected_item_id() {
                    self.state.push_view(View::Logs { target_id: id });
                }
            }
            (KeyCode::Char('d'), _) => {
                // Describe selected item
                self.handle_describe();
            }
            (KeyCode::Char('x'), _) => {
                // Cancel selected execution
                self.handle_cancel();
            }
            (KeyCode::Char('p'), _) => {
                // In REPL Chat mode: switch to Plan mode
                // Otherwise: Pause selected execution
                if matches!(self.state.current_view, View::Repl)
                    && self.state.repl_mode == ReplMode::Chat
                    && !self.state.repl_streaming
                {
                    self.state.repl_mode = ReplMode::Plan;
                } else {
                    self.handle_pause();
                }
            }
            (KeyCode::Char('c'), _) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                // In REPL Plan mode: switch to Chat mode
                if matches!(self.state.current_view, View::Repl)
                    && self.state.repl_mode == ReplMode::Plan
                    && !self.state.repl_streaming
                {
                    self.state.repl_mode = ReplMode::Chat;
                }
            }
            (KeyCode::Char('r'), _) => {
                // Resume selected execution
                self.handle_resume();
            }
            (KeyCode::Char('D'), _) => {
                // Delete selected execution
                self.handle_delete();
            }

            // === New task ===
            (KeyCode::Char('n'), _) if matches!(self.state.current_view, View::Executions) => {
                self.state.interaction_mode = InteractionMode::TaskInput(String::new());
            }

            // === Logs view specific ===
            (KeyCode::Char('f'), _) if matches!(self.state.current_view, View::Logs { .. }) => {
                self.state.logs_follow = !self.state.logs_follow;
            }

            // === Tab toggles Chat/Plan mode in REPL view ===
            (KeyCode::Tab, _) if matches!(self.state.current_view, View::Repl) && !self.state.repl_streaming => {
                self.state.repl_mode = match self.state.repl_mode {
                    ReplMode::Chat => ReplMode::Plan,
                    ReplMode::Plan => ReplMode::Chat,
                };
            }

            // === REPL view specific: any other character starts input ===
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT)
                if matches!(self.state.current_view, View::Repl) && !self.state.repl_streaming =>
            {
                self.state.repl_input.push(c);
                self.state.interaction_mode = InteractionMode::ReplInput;
            }

            _ => {}
        }

        false
    }

    /// Navigate to the previous top-level view
    fn navigate_prev_top_level_view(&mut self) {
        let current_idx = top_level_view_index(&self.state.current_view);
        let prev_idx = if current_idx == 0 { TOP_LEVEL_VIEWS.len() - 1 } else { current_idx - 1 };
        self.state.current_view = TOP_LEVEL_VIEWS[prev_idx].clone();
        self.state.view_stack.clear();
    }

    /// Navigate to the next top-level view
    fn navigate_next_top_level_view(&mut self) {
        let current_idx = top_level_view_index(&self.state.current_view);
        let next_idx = (current_idx + 1) % TOP_LEVEL_VIEWS.len();
        self.state.current_view = TOP_LEVEL_VIEWS[next_idx].clone();
        self.state.view_stack.clear();
    }

    /// Handle drill down (Enter key)
    fn handle_drill_down(&mut self) {
        match &self.state.current_view {
            View::Records { .. } => {
                // Drill into children for selected record or show describe
                if let (Some(id), Some(loop_type)) = (self.state.selected_item_id(), self.state.selected_item_type()) {
                    // Navigate to children records filtered by parent
                    self.state.push_view(View::Records {
                        type_filter: None,
                        parent_filter: Some(id),
                    });
                    // Store the loop_type for context (available_types should have children)
                    let _ = loop_type; // Used for context
                }
            }
            View::Executions => {
                // Show describe for selected execution
                if let (Some(id), Some(loop_type)) = (self.state.selected_item_id(), self.state.selected_item_type()) {
                    self.state.push_view(View::Describe {
                        target_id: id,
                        target_type: loop_type,
                    });
                }
            }
            _ => {}
        }
    }

    /// Handle escape key
    fn handle_escape(&mut self) {
        // Clear filter first if active
        if !self.state.filter_text.is_empty() {
            self.state.filter_text.clear();
            return;
        }

        // Then try to pop view stack
        if self.state.pop_view() {
            return;
        }

        // If at root view, prompt to quit if executions are active
        if self.state.executions_active > 0 {
            self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
        } else {
            self.state.should_quit = true;
        }
    }

    /// Handle describe action
    fn handle_describe(&mut self) {
        if let (Some(id), Some(loop_type)) = (self.state.selected_item_id(), self.state.selected_item_type()) {
            self.state.push_view(View::Describe {
                target_id: id,
                target_type: loop_type,
            });
        }
    }

    /// Handle cancel action
    fn handle_cancel(&mut self) {
        if !matches!(self.state.current_view, View::Executions) {
            return;
        }

        if let (Some(id), Some(name)) = (self.state.selected_item_id(), self.state.selected_item_name()) {
            let filtered = self.state.filtered_executions();
            if let Some(exec_item) = filtered.get(self.state.executions_selection.selected_index)
                && !["complete", "failed", "cancelled", "stopped"].contains(&exec_item.status.as_str())
            {
                self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::cancel_loop(id, &name));
            }
        }
    }

    /// Handle pause action
    fn handle_pause(&mut self) {
        if !matches!(self.state.current_view, View::Executions) {
            return;
        }

        if let (Some(id), Some(name)) = (self.state.selected_item_id(), self.state.selected_item_name()) {
            let filtered = self.state.filtered_executions();
            if let Some(exec_item) = filtered.get(self.state.executions_selection.selected_index)
                && exec_item.status == "running"
            {
                self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::pause_loop(id, &name));
            }
        }
    }

    /// Handle resume action
    fn handle_resume(&mut self) {
        if !matches!(self.state.current_view, View::Executions) {
            return;
        }

        if let (Some(id), Some(name)) = (self.state.selected_item_id(), self.state.selected_item_name()) {
            let filtered = self.state.filtered_executions();
            if let Some(exec_item) = filtered.get(self.state.executions_selection.selected_index)
                && exec_item.status == "paused"
            {
                self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                    ConfirmAction::ResumeLoop(id),
                    format!("Resume {}?", name),
                ));
            }
        }
    }

    /// Handle delete action
    fn handle_delete(&mut self) {
        if !matches!(self.state.current_view, View::Executions) {
            return;
        }

        if let (Some(id), Some(name)) = (self.state.selected_item_id(), self.state.selected_item_name()) {
            self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::delete_execution(id, &name));
        }
    }

    /// Handle key in filter mode
    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                // Apply the filter
                if let InteractionMode::Filter(text) = &self.state.interaction_mode {
                    self.state.filter_text = text.clone();
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

    /// Handle key in task input mode
    fn handle_task_input_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                // Submit the task (store it for runner to process)
                if let InteractionMode::TaskInput(task) = &self.state.interaction_mode
                    && !task.trim().is_empty()
                {
                    self.state.pending_task = Some(task.clone());
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

    /// Handle key in REPL input mode
    fn handle_repl_input_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                // Clear input and return to normal mode
                self.state.repl_input.clear();
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                // Submit the input for processing
                let input = std::mem::take(&mut self.state.repl_input);
                if !input.trim().is_empty() {
                    // Handle slash commands
                    if input.starts_with('/') {
                        self.handle_repl_slash_command(&input);
                    } else {
                        // Queue for LLM processing
                        self.state.pending_repl_submit = Some(input);
                    }
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Backspace => {
                self.state.repl_input.pop();
                // If input is empty, return to normal mode
                if self.state.repl_input.is_empty() {
                    self.state.interaction_mode = InteractionMode::Normal;
                }
            }
            KeyCode::Char(c) => {
                self.state.repl_input.push(c);
            }
            // Tab toggles Chat/Plan mode
            KeyCode::Tab => {
                self.state.repl_mode = match self.state.repl_mode {
                    ReplMode::Chat => ReplMode::Plan,
                    ReplMode::Plan => ReplMode::Chat,
                };
            }
            // Allow view navigation with arrow keys even in input mode
            KeyCode::Left => {
                self.state.interaction_mode = InteractionMode::Normal;
                self.navigate_prev_top_level_view();
            }
            KeyCode::Right => {
                self.state.interaction_mode = InteractionMode::Normal;
                self.navigate_next_top_level_view();
            }
            _ => {}
        }

        false
    }

    /// Handle REPL slash commands
    fn handle_repl_slash_command(&mut self, input: &str) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");

        match cmd {
            "/help" | "/h" => {
                self.state.interaction_mode = InteractionMode::Help;
            }
            "/quit" | "/q" | "/exit" => {
                if self.state.executions_active > 0 {
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
                } else {
                    self.state.should_quit = true;
                }
            }
            "/clear" | "/c" => {
                self.state.repl_history.clear();
                self.state.repl_response_buffer.clear();
                self.state.repl_scroll = 0;
            }
            "/executions" | "/exec" => {
                self.state.current_view = View::Executions;
                self.state.view_stack.clear();
            }
            "/records" | "/rec" => {
                self.state.current_view = View::Records {
                    type_filter: None,
                    parent_filter: None,
                };
                self.state.view_stack.clear();
            }
            _ => {
                self.state.set_error(format!("Unknown command: {}", cmd));
            }
        }
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
                    // User confirmed - queue action for runner to execute
                    match &dialog.action {
                        ConfirmAction::Quit => {
                            self.state.should_quit = true;
                        }
                        ConfirmAction::CancelLoop(id) => {
                            self.state.pending_action = Some(PendingAction::CancelLoop(id.clone()));
                        }
                        ConfirmAction::PauseLoop(id) => {
                            self.state.pending_action = Some(PendingAction::PauseLoop(id.clone()));
                        }
                        ConfirmAction::ResumeLoop(id) => {
                            self.state.pending_action = Some(PendingAction::ResumeLoop(id.clone()));
                        }
                        ConfirmAction::DeleteExecution(id) => {
                            self.state.pending_action = Some(PendingAction::DeleteExecution(id.clone()));
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

        // Try view switching first (delegates to View::from_command which defines available views)
        if let Some(view) = View::from_command(command, &self.state.available_types) {
            self.state.navigate_to(view);
            return;
        }

        // Handle other commands
        match command {
            // Create new task - enter task input mode
            "new" => {
                self.state.navigate_to(View::Executions); // Switch to executions view
                self.state.interaction_mode = InteractionMode::TaskInput(String::new());
            }

            // Quit
            "quit" | "q" => {
                if self.state.executions_active > 0 {
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
        // Default view is now REPL
        assert!(matches!(app.state().current_view, View::Repl));
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
    fn test_app_filter_mode() {
        let mut app = App::new();

        // Enter filter mode
        let key = KeyEvent::from(KeyCode::Char('/'));
        app.handle_key(key);
        assert!(matches!(app.state().interaction_mode, InteractionMode::Filter(_)));

        // Type some text
        app.handle_key(KeyEvent::from(KeyCode::Char('t')));
        app.handle_key(KeyEvent::from(KeyCode::Char('e')));
        app.handle_key(KeyEvent::from(KeyCode::Char('s')));
        app.handle_key(KeyEvent::from(KeyCode::Char('t')));

        if let InteractionMode::Filter(text) = &app.state().interaction_mode {
            assert_eq!(text, "test");
        }

        // Press Enter to apply
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(app.state().interaction_mode, InteractionMode::Normal));
        assert_eq!(app.state().filter_text, "test");
    }

    #[test]
    fn test_app_command_mode() {
        let mut app = App::new();

        // Enter command mode
        let key = KeyEvent::from(KeyCode::Char(':'));
        app.handle_key(key);
        assert!(matches!(app.state().interaction_mode, InteractionMode::Command(_)));

        // Type "records"
        for c in "records".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }

        // Press Enter to execute
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(app.state().current_view, View::Records { .. }));
    }

    #[test]
    fn test_execute_command_view_switch() {
        let mut app = App::new();

        app.execute_command("records".to_string());
        assert!(matches!(app.state().current_view, View::Records { .. }));

        app.execute_command("loops".to_string());
        assert!(matches!(app.state().current_view, View::Executions));

        app.execute_command("executions".to_string());
        assert!(matches!(app.state().current_view, View::Executions));
    }

    #[test]
    fn test_execute_command_unknown() {
        let mut app = App::new();

        app.execute_command("unknowncommand".to_string());
        assert!(app.state().error_message.is_some());
        assert!(app.state().error_message.as_ref().unwrap().contains("Unknown command"));
    }
}
