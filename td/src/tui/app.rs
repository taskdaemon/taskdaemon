//! TUI application - event handling and state management
//!
//! The App struct owns the AppState and handles all keyboard events.
//! It does not do any rendering - that's delegated to the views module.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::{debug, info, trace, warn};

use super::state::{
    AppState, ConfirmAction, ConfirmDialog, InteractionMode, PendingAction, PlanCreateRequest, ReplMessage, ReplMode,
    TopLevelPane, View, current_pane,
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
        debug!("App::new: called");
        Self { state: AppState::new() }
    }

    /// Get reference to state
    pub fn state(&self) -> &AppState {
        trace!("App::state: called");
        &self.state
    }

    /// Get mutable reference to state
    pub fn state_mut(&mut self) -> &mut AppState {
        trace!("App::state_mut: called");
        &mut self.state
    }

    /// Handle a key event
    ///
    /// Returns true if the application should exit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_key: called");
        // Clear any transient error message on key press
        self.state.clear_error();

        // Handle based on interaction mode
        match &self.state.interaction_mode {
            InteractionMode::Normal => {
                debug!("App::handle_key: Normal mode");
                self.handle_normal_key(key)
            }
            InteractionMode::Filter(_) => {
                debug!("App::handle_key: Filter mode");
                self.handle_filter_key(key)
            }
            InteractionMode::Command(_) => {
                debug!("App::handle_key: Command mode");
                self.handle_command_key(key)
            }
            InteractionMode::TaskInput(_) => {
                debug!("App::handle_key: TaskInput mode");
                self.handle_task_input_key(key)
            }
            InteractionMode::ReplInput => {
                debug!("App::handle_key: ReplInput mode");
                self.handle_repl_input_key(key)
            }
            InteractionMode::Confirm(_) => {
                debug!("App::handle_key: Confirm mode");
                self.handle_confirm_key(key)
            }
            InteractionMode::Help => {
                debug!("App::handle_key: Help mode");
                self.handle_help_key(key)
            }
        }
    }

    /// Handle key in normal mode
    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_normal_key: called");
        match (key.code, key.modifiers) {
            // === Quit ===
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                debug!("App::handle_normal_key: Ctrl+C force quit");
                return true; // Force quit
            }
            (KeyCode::Char('q'), _) => {
                debug!("App::handle_normal_key: quit requested");
                if self.state.executions_active > 0 {
                    debug!("App::handle_normal_key: showing quit confirm dialog");
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
                } else {
                    debug!("App::handle_normal_key: no active executions, quitting");
                    self.state.should_quit = true;
                }
            }

            // === Help ===
            (KeyCode::Char('?'), _) | (KeyCode::F(1), _) => {
                debug!("App::handle_normal_key: showing help");
                self.state.interaction_mode = InteractionMode::Help;
            }

            // === Mode switching ===
            // Note: In REPL view, '/' and ':' are handled by REPL input (see below)
            (KeyCode::Char('/'), _) if !matches!(self.state.current_view, View::Repl) => {
                debug!("App::handle_normal_key: entering filter mode");
                self.state.interaction_mode = InteractionMode::Filter(String::new());
            }
            (KeyCode::Char(':'), _) if !matches!(self.state.current_view, View::Repl) => {
                debug!("App::handle_normal_key: entering command mode");
                self.state.interaction_mode = InteractionMode::Command(String::new());
            }

            // === Top-level view navigation (Tab cycles, C/P/E/R jump) ===
            (KeyCode::Tab, _) => {
                debug!("App::handle_normal_key: Tab - next view");
                self.navigate_next_top_level_view();
            }
            (KeyCode::BackTab, _) => {
                debug!("App::handle_normal_key: BackTab - prev view");
                // Shift+Tab goes backwards
                self.navigate_prev_top_level_view();
            }
            (KeyCode::Char('C'), _) => {
                debug!("App::handle_normal_key: navigate to Chat pane");
                self.navigate_to_pane(TopLevelPane::Chat);
            }
            (KeyCode::Char('P'), _) => {
                debug!("App::handle_normal_key: navigate to Plan pane");
                self.navigate_to_pane(TopLevelPane::Plan);
            }
            (KeyCode::Char('L'), _) if !matches!(self.state.current_view, View::Loops) => {
                debug!("App::handle_normal_key: navigate to Loops pane");
                self.navigate_to_pane(TopLevelPane::Loops);
            }

            // === Navigation (list views) or Scroll (REPL/Describe view) ===
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                debug!("App::handle_normal_key: up/k navigation");
                if matches!(self.state.current_view, View::Repl) {
                    debug!("App::handle_normal_key: scroll up in REPL");
                    // Scroll up in REPL view
                    let max = self.state.repl_max_scroll;
                    self.state.repl_scroll_up(1, max);
                } else if matches!(self.state.current_view, View::Describe { .. }) {
                    debug!("App::handle_normal_key: scroll up in Describe");
                    // Scroll up in Describe view
                    self.state.describe_scroll_up(1);
                } else if matches!(self.state.current_view, View::Loops) {
                    debug!("App::handle_normal_key: select prev in Loops tree");
                    // Tree navigation for Loops view
                    self.state.loops_tree.select_prev();
                } else if let Some(sel) = self.state.current_selection_mut() {
                    debug!("App::handle_normal_key: select prev in list");
                    sel.select_prev();
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                debug!("App::handle_normal_key: down/j navigation");
                if matches!(self.state.current_view, View::Repl) {
                    debug!("App::handle_normal_key: scroll down in REPL");
                    // Scroll down in REPL view
                    let max = self.state.repl_max_scroll;
                    self.state.repl_scroll_down(1, max);
                } else if matches!(self.state.current_view, View::Describe { .. }) {
                    debug!("App::handle_normal_key: scroll down in Describe");
                    // Scroll down in Describe view
                    self.state.describe_scroll_down(1);
                } else if matches!(self.state.current_view, View::Loops) {
                    debug!("App::handle_normal_key: select next in Loops tree");
                    // Tree navigation for Loops view
                    self.state.loops_tree.select_next();
                } else {
                    debug!("App::handle_normal_key: select next in list");
                    let max = self.state.current_item_count();
                    if let Some(sel) = self.state.current_selection_mut() {
                        sel.select_next(max);
                    }
                }
            }
            (KeyCode::PageUp, _) => {
                debug!("App::handle_normal_key: PageUp");
                if matches!(self.state.current_view, View::Repl) {
                    debug!("App::handle_normal_key: page up in REPL");
                    let max = self.state.repl_max_scroll;
                    self.state.repl_scroll_up(10, max);
                } else if matches!(self.state.current_view, View::Describe { .. }) {
                    debug!("App::handle_normal_key: page up in Describe");
                    self.state.describe_scroll_up(10);
                }
            }
            (KeyCode::PageDown, _) => {
                debug!("App::handle_normal_key: PageDown");
                if matches!(self.state.current_view, View::Repl) {
                    debug!("App::handle_normal_key: page down in REPL");
                    let max = self.state.repl_max_scroll;
                    self.state.repl_scroll_down(10, max);
                } else if matches!(self.state.current_view, View::Describe { .. }) {
                    debug!("App::handle_normal_key: page down in Describe");
                    self.state.describe_scroll_down(10);
                }
            }
            (KeyCode::Char('g'), _) => {
                debug!("App::handle_normal_key: g - go to top");
                if matches!(self.state.current_view, View::Repl) {
                    debug!("App::handle_normal_key: scroll to top in REPL");
                    // Scroll to top
                    self.state.repl_scroll = Some(0);
                } else if matches!(self.state.current_view, View::Describe { .. }) {
                    debug!("App::handle_normal_key: scroll to top in Describe");
                    self.state.describe_scroll_to_top();
                } else if matches!(self.state.current_view, View::Loops) {
                    debug!("App::handle_normal_key: select first in Loops tree");
                    self.state.loops_tree.select_first();
                } else if let Some(sel) = self.state.current_selection_mut() {
                    debug!("App::handle_normal_key: select first in list");
                    sel.select_first();
                }
            }
            (KeyCode::Char('G'), _) => {
                debug!("App::handle_normal_key: G - go to bottom");
                if matches!(self.state.current_view, View::Repl) {
                    debug!("App::handle_normal_key: scroll to bottom in REPL");
                    // Scroll to bottom (auto-scroll mode)
                    self.state.repl_scroll_to_bottom();
                } else if matches!(self.state.current_view, View::Describe { .. }) {
                    debug!("App::handle_normal_key: scroll to bottom in Describe");
                    // Scroll to bottom in Describe view
                    self.state.describe_scroll = self.state.describe_max_scroll;
                } else if matches!(self.state.current_view, View::Loops) {
                    debug!("App::handle_normal_key: select last in Loops tree");
                    self.state.loops_tree.select_last();
                } else {
                    debug!("App::handle_normal_key: select last in list");
                    let max = self.state.current_item_count();
                    if let Some(sel) = self.state.current_selection_mut() {
                        sel.select_last(max);
                    }
                }
            }

            // === Drill down / back ===
            (KeyCode::Enter, _) => {
                debug!("App::handle_normal_key: Enter - drill down");
                self.handle_drill_down();
            }
            (KeyCode::Esc, _) => {
                debug!("App::handle_normal_key: Esc - escape");
                self.handle_escape();
            }

            // === Loops tree expand/collapse ===
            (KeyCode::Right, _) if matches!(self.state.current_view, View::Loops) => {
                debug!("App::handle_normal_key: Right - expand in Loops");
                self.state.loops_tree.expand_selected();
            }
            (KeyCode::Left, _) if matches!(self.state.current_view, View::Loops) => {
                debug!("App::handle_normal_key: Left - collapse in Loops");
                self.state.loops_tree.collapse_selected();
            }
            (KeyCode::Char('h'), _) if matches!(self.state.current_view, View::Loops) => {
                debug!("App::handle_normal_key: h - collapse in Loops");
                self.state.loops_tree.collapse_selected();
            }
            (KeyCode::Char('l'), _) if matches!(self.state.current_view, View::Loops) => {
                debug!("App::handle_normal_key: l - expand in Loops");
                self.state.loops_tree.expand_selected();
            }
            // Loops view: [o] Output - go to Describe with output shown
            (KeyCode::Char('o'), _) if matches!(self.state.current_view, View::Loops) => {
                debug!("App::handle_normal_key: o - show output in Loops");
                if let (Some(id), Some(loop_type)) = (self.state.selected_item_id(), self.state.selected_item_type()) {
                    self.state.describe_show_output = true;
                    self.state.push_view(View::Describe {
                        target_id: id,
                        target_type: loop_type,
                    });
                }
            }
            // Loops view: [L] Logs - go to Logs view
            (KeyCode::Char('L'), _) if matches!(self.state.current_view, View::Loops) => {
                debug!("App::handle_normal_key: L - view logs in Loops");
                if let Some(id) = self.state.selected_item_id() {
                    self.state.push_view(View::Logs { target_id: id });
                }
            }

            // === List view actions (Executions, Records) - not in REPL, Loops, or Describe ===
            (KeyCode::Char('l'), _)
                if !matches!(
                    self.state.current_view,
                    View::Repl | View::Loops | View::Describe { .. }
                ) =>
            {
                debug!("App::handle_normal_key: l - view logs");
                // View logs for selected item
                if let Some(id) = self.state.selected_item_id() {
                    self.state.push_view(View::Logs { target_id: id });
                }
            }
            (KeyCode::Char('d'), _)
                if matches!(
                    self.state.current_view,
                    View::Executions | View::Records { .. } | View::Loops
                ) =>
            {
                debug!("App::handle_normal_key: d - describe");
                // Describe selected item
                self.handle_describe();
            }
            (KeyCode::Char('x'), _) if matches!(self.state.current_view, View::Executions | View::Loops) => {
                debug!("App::handle_normal_key: x - cancel");
                // Cancel selected execution
                self.handle_cancel();
            }
            (KeyCode::Char('s'), _) if matches!(self.state.current_view, View::Executions | View::Loops) => {
                debug!("App::handle_normal_key: s - toggle state");
                // Toggle state: draft->pending, pending->paused, paused->pending, running->paused
                info!("'s' key pressed in {:?} view", self.state.current_view);
                self.handle_toggle_state();
            }
            (KeyCode::Char('D'), _) if matches!(self.state.current_view, View::Executions | View::Loops) => {
                debug!("App::handle_normal_key: D - delete");
                // Delete selected execution
                self.handle_delete();
            }

            // === New task ===
            (KeyCode::Char('n'), _) if matches!(self.state.current_view, View::Executions) => {
                debug!("App::handle_normal_key: n - new task");
                self.state.interaction_mode = InteractionMode::TaskInput(String::new());
            }

            // === Logs view specific ===
            (KeyCode::Char('f'), _) if matches!(self.state.current_view, View::Logs { .. }) => {
                debug!("App::handle_normal_key: f - toggle logs follow");
                self.state.logs_follow = !self.state.logs_follow;
            }

            // === Describe view specific: show output ===
            (KeyCode::Char('o'), _) if matches!(self.state.current_view, View::Describe { .. }) => {
                debug!("App::handle_normal_key: o - show output in Describe");
                self.handle_show_output();
            }

            // === Describe view specific: toggle state ===
            (KeyCode::Char('s'), _) if matches!(self.state.current_view, View::Describe { .. }) => {
                debug!("App::handle_normal_key: s - toggle state in Describe");
                info!("'s' key pressed in Describe view");
                self.handle_toggle_state_describe();
            }

            // === Describe view specific: show logs ===
            (KeyCode::Char('l'), _) if matches!(self.state.current_view, View::Describe { .. }) => {
                debug!("App::handle_normal_key: l - show logs in Describe");
                self.handle_show_logs_describe();
            }

            // === REPL view specific: toggle tool output expansion ===
            (KeyCode::Char('o'), KeyModifiers::NONE) if matches!(self.state.current_view, View::Repl) => {
                debug!("App::handle_normal_key: o - toggle tool expansion in REPL");
                self.state.toggle_tool_expansion();
            }
            (KeyCode::Char('o'), KeyModifiers::CONTROL) if matches!(self.state.current_view, View::Repl) => {
                debug!("App::handle_normal_key: Ctrl+o - toggle tool expansion in REPL");
                self.state.toggle_tool_expansion();
            }

            // === REPL view specific: any other character starts input ===
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT)
                if matches!(self.state.current_view, View::Repl) && !self.state.repl_streaming =>
            {
                debug!(%c, "App::handle_normal_key: char starts REPL input");
                self.state.repl_input.push(c);
                self.state.repl_cursor_pos = self.state.repl_input.len();
                self.state.interaction_mode = InteractionMode::ReplInput;
            }

            _ => {
                debug!("App::handle_normal_key: unhandled key");
            }
        }

        false
    }

    /// Navigate to the previous top-level pane
    fn navigate_prev_top_level_view(&mut self) {
        debug!("App::navigate_prev_top_level_view: called");
        let current = current_pane(&self.state.current_view, self.state.repl_mode);
        self.navigate_to_pane(current.prev());
    }

    /// Navigate to the next top-level pane
    fn navigate_next_top_level_view(&mut self) {
        debug!("App::navigate_next_top_level_view: called");
        let current = current_pane(&self.state.current_view, self.state.repl_mode);
        self.navigate_to_pane(current.next());
    }

    /// Navigate to a specific pane
    fn navigate_to_pane(&mut self, pane: TopLevelPane) {
        debug!(?pane, "App::navigate_to_pane: called");
        match pane {
            TopLevelPane::Chat => {
                debug!("App::navigate_to_pane: switching to Chat");
                self.state.current_view = View::Repl;
                self.state.repl_mode = ReplMode::Chat;
            }
            TopLevelPane::Plan => {
                debug!("App::navigate_to_pane: switching to Plan");
                self.state.current_view = View::Repl;
                self.state.repl_mode = ReplMode::Plan;
            }
            TopLevelPane::Loops => {
                debug!("App::navigate_to_pane: switching to Loops");
                self.state.current_view = View::Loops;
            }
        }
        self.state.view_stack.clear();
    }

    /// Handle drill down (Enter key)
    fn handle_drill_down(&mut self) {
        debug!("App::handle_drill_down: called");
        match &self.state.current_view {
            View::Loops => {
                debug!("App::handle_drill_down: in Loops view");
                // Show describe for selected execution (use arrow keys for expand/collapse)
                if let (Some(id), Some(loop_type)) = (self.state.selected_item_id(), self.state.selected_item_type()) {
                    debug!(%id, %loop_type, "App::handle_drill_down: pushing Describe view");
                    self.state.push_view(View::Describe {
                        target_id: id,
                        target_type: loop_type,
                    });
                }
            }
            View::Records { .. } => {
                debug!("App::handle_drill_down: in Records view");
                // Drill into children for selected record or show describe
                if let (Some(id), Some(loop_type)) = (self.state.selected_item_id(), self.state.selected_item_type()) {
                    debug!(%id, %loop_type, "App::handle_drill_down: pushing child Records view");
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
                debug!("App::handle_drill_down: in Executions view");
                // Show describe for selected execution
                if let (Some(id), Some(loop_type)) = (self.state.selected_item_id(), self.state.selected_item_type()) {
                    debug!(%id, %loop_type, "App::handle_drill_down: pushing Describe view");
                    self.state.push_view(View::Describe {
                        target_id: id,
                        target_type: loop_type,
                    });
                }
            }
            _ => {
                debug!("App::handle_drill_down: no action for current view");
            }
        }
    }

    /// Handle escape key
    fn handle_escape(&mut self) {
        debug!("App::handle_escape: called");
        // Clear filter first if active
        if !self.state.filter_text.is_empty() {
            debug!("App::handle_escape: clearing filter");
            self.state.filter_text.clear();
            return;
        }

        // Then try to pop view stack
        if self.state.pop_view() {
            debug!("App::handle_escape: popped view from stack");
            return;
        }

        // If at root view, prompt to quit if executions are active
        if self.state.executions_active > 0 {
            debug!("App::handle_escape: showing quit confirm dialog");
            self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
        } else {
            debug!("App::handle_escape: quitting");
            self.state.should_quit = true;
        }
    }

    /// Handle describe action
    fn handle_describe(&mut self) {
        debug!("App::handle_describe: called");
        // Get selected item from tree if in Loops view, otherwise use standard selection
        let (id, loop_type) = if matches!(self.state.current_view, View::Loops) {
            debug!("App::handle_describe: getting selection from Loops tree");
            self.state
                .loops_tree
                .selected_node()
                .map(|n| (n.item.id.clone(), n.item.loop_type.clone()))
                .unzip()
        } else {
            debug!("App::handle_describe: getting selection from standard list");
            (self.state.selected_item_id(), self.state.selected_item_type())
        };

        if let (Some(id), Some(loop_type)) = (id, loop_type) {
            debug!(%id, %loop_type, "App::handle_describe: pushing Describe view");
            self.state.push_view(View::Describe {
                target_id: id,
                target_type: loop_type,
            });
        } else {
            debug!("App::handle_describe: no item selected");
        }
    }

    /// Handle cancel action
    fn handle_cancel(&mut self) {
        debug!("App::handle_cancel: called");
        // Get selected item from tree if in Loops view, otherwise use standard selection
        let selected = if matches!(self.state.current_view, View::Loops) {
            debug!("App::handle_cancel: getting selection from Loops tree");
            self.state.loops_tree.selected_node().map(|n| n.item.clone())
        } else if matches!(self.state.current_view, View::Executions) {
            debug!("App::handle_cancel: getting selection from Executions list");
            let filtered = self.state.filtered_executions();
            filtered
                .get(self.state.executions_selection.selected_index)
                .copied()
                .cloned()
        } else {
            debug!("App::handle_cancel: not in cancellable view");
            return;
        };

        if let Some(item) = selected
            && !["complete", "failed", "cancelled", "stopped"].contains(&item.status.as_str())
        {
            debug!(%item.id, %item.status, "App::handle_cancel: showing cancel confirm dialog");
            self.state.interaction_mode =
                InteractionMode::Confirm(ConfirmDialog::cancel_loop(item.id.clone(), &item.name));
        } else {
            debug!("App::handle_cancel: item not cancellable or no selection");
        }
    }

    /// Handle pause action
    fn handle_pause(&mut self) {
        debug!("App::handle_pause: called");
        // Get selected item from tree if in Loops view, otherwise use standard selection
        let selected = if matches!(self.state.current_view, View::Loops) {
            debug!("App::handle_pause: getting selection from Loops tree");
            self.state.loops_tree.selected_node().map(|n| n.item.clone())
        } else if matches!(self.state.current_view, View::Executions) {
            debug!("App::handle_pause: getting selection from Executions list");
            let filtered = self.state.filtered_executions();
            filtered
                .get(self.state.executions_selection.selected_index)
                .copied()
                .cloned()
        } else {
            debug!("App::handle_pause: not in pausable view");
            return;
        };

        if let Some(item) = selected
            && item.status == "running"
        {
            debug!(%item.id, "App::handle_pause: showing pause confirm dialog");
            self.state.interaction_mode =
                InteractionMode::Confirm(ConfirmDialog::pause_loop(item.id.clone(), &item.name));
        } else {
            debug!("App::handle_pause: item not running or no selection");
        }
    }

    /// Handle resume action
    fn handle_resume(&mut self) {
        debug!("App::handle_resume: called");
        // Get selected item from tree if in Loops view, otherwise use standard selection
        let selected = if matches!(self.state.current_view, View::Loops) {
            debug!("App::handle_resume: getting selection from Loops tree");
            self.state.loops_tree.selected_node().map(|n| n.item.clone())
        } else if matches!(self.state.current_view, View::Executions) {
            debug!("App::handle_resume: getting selection from Executions list");
            let filtered = self.state.filtered_executions();
            filtered
                .get(self.state.executions_selection.selected_index)
                .copied()
                .cloned()
        } else {
            debug!("App::handle_resume: not in resumable view");
            return;
        };

        if let Some(item) = selected
            && item.status == "paused"
        {
            debug!(%item.id, "App::handle_resume: showing resume confirm dialog");
            self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                ConfirmAction::ResumeLoop(item.id.clone()),
                format!("Resume {}?", item.name),
            ));
        } else {
            debug!("App::handle_resume: item not paused or no selection");
        }
    }

    /// Handle delete action
    fn handle_delete(&mut self) {
        debug!("App::handle_delete: called");
        // Get selected item from tree if in Loops view, otherwise use standard selection
        let selected = if matches!(self.state.current_view, View::Loops) {
            debug!("App::handle_delete: getting selection from Loops tree");
            self.state.loops_tree.selected_node().map(|n| n.item.clone())
        } else if matches!(self.state.current_view, View::Executions) {
            debug!("App::handle_delete: getting selection from Executions list");
            let filtered = self.state.filtered_executions();
            filtered
                .get(self.state.executions_selection.selected_index)
                .copied()
                .cloned()
        } else {
            debug!("App::handle_delete: not in deletable view");
            return;
        };

        if let Some(item) = selected {
            debug!(%item.id, "App::handle_delete: showing delete confirm dialog");
            self.state.interaction_mode =
                InteractionMode::Confirm(ConfirmDialog::delete_execution(item.id.clone(), &item.name));
        } else {
            debug!("App::handle_delete: no item selected");
        }
    }

    /// Handle start draft action (transitions Draft -> Pending)
    fn handle_start_draft(&mut self) {
        debug!("App::handle_start_draft: called");
        // Get selected item from tree if in Loops view, otherwise use standard selection
        let selected = if matches!(self.state.current_view, View::Loops) {
            debug!("App::handle_start_draft: getting selection from Loops tree");
            self.state.loops_tree.selected_node().map(|n| n.item.clone())
        } else if matches!(self.state.current_view, View::Executions) {
            debug!("App::handle_start_draft: getting selection from Executions list");
            let filtered = self.state.filtered_executions();
            filtered
                .get(self.state.executions_selection.selected_index)
                .copied()
                .cloned()
        } else {
            debug!("App::handle_start_draft: not in startable view");
            return;
        };

        if let Some(item) = selected
            && item.status == "draft"
        {
            debug!(%item.id, "App::handle_start_draft: showing start confirm dialog");
            self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::new(
                ConfirmAction::ActivateDraft(item.id.clone()),
                format!("Start execution of {}?", item.name),
            ));
        } else {
            debug!("App::handle_start_draft: item not draft or no selection");
        }
    }

    /// Handle toggle state action - cycles through states based on current state
    /// draft -> pending (start), pending -> paused, paused -> pending (resume), running -> paused
    fn handle_toggle_state(&mut self) {
        debug!("handle_toggle_state called, view={:?}", self.state.current_view);

        // Get selected item from tree if in Loops view, otherwise use standard selection
        let selected = if matches!(self.state.current_view, View::Loops) {
            let sel = self.state.loops_tree.selected_node().map(|n| n.item.clone());
            debug!("Loops view selection: {:?}", sel.as_ref().map(|i| (&i.id, &i.status)));
            sel
        } else if matches!(self.state.current_view, View::Executions) {
            let filtered = self.state.filtered_executions();
            let sel = filtered
                .get(self.state.executions_selection.selected_index)
                .copied()
                .cloned();
            debug!(
                "Executions view selection: {:?}",
                sel.as_ref().map(|i| (&i.id, &i.status))
            );
            sel
        } else {
            debug!("Not in Loops or Executions view, ignoring");
            return;
        };

        if let Some(item) = selected {
            info!("Toggle state for '{}' (status={})", item.id, item.status);
            // State transitions: draft->running, running->paused, paused->running
            // NO PENDING - draft goes directly to running (active)
            let action = match item.status.as_str() {
                "draft" => {
                    info!("Setting ActivateDraft action (draft -> running)");
                    PendingAction::ActivateDraft(item.id.clone())
                }
                "running" => {
                    info!("Setting PauseLoop action (running -> paused)");
                    PendingAction::PauseLoop(item.id.clone())
                }
                "paused" => {
                    info!("Setting ResumeLoop action (paused -> running)");
                    PendingAction::ResumeLoop(item.id.clone())
                }
                status => {
                    warn!("Cannot toggle status '{}' - no action taken", status);
                    return;
                }
            };

            self.state.pending_action = Some(action.clone());
            info!("pending_action set to {:?}", action);
        } else {
            warn!("No item selected, cannot toggle state");
        }
    }

    /// Handle show output action in Describe view - toggles between plan and output
    fn handle_show_output(&mut self) {
        debug!("App::handle_show_output: called");
        if matches!(self.state.current_view, View::Describe { .. }) {
            debug!("App::handle_show_output: toggling output view");
            self.state.describe_show_output = !self.state.describe_show_output;
            self.state.describe_scroll = 0; // Reset scroll when toggling
        } else {
            debug!("App::handle_show_output: not in Describe view");
        }
    }

    /// Handle state toggle in Describe view
    fn handle_toggle_state_describe(&mut self) {
        debug!("handle_toggle_state_describe called");
        if let View::Describe { ref target_id, .. } = self.state.current_view {
            debug!("Looking for execution with id={}", target_id);
            // Find the execution from executions list
            if let Some(item) = self.state.executions.iter().find(|e| &e.id == target_id) {
                info!("Found execution '{}' with status '{}'", item.id, item.status);
                // State transitions: draft->running, running->paused, paused->running
                let action = match item.status.as_str() {
                    "draft" => {
                        info!("Setting ActivateDraft action (draft -> running)");
                        PendingAction::ActivateDraft(item.id.clone())
                    }
                    "running" => {
                        info!("Setting PauseLoop action (running -> paused)");
                        PendingAction::PauseLoop(item.id.clone())
                    }
                    "paused" => {
                        info!("Setting ResumeLoop action (paused -> running)");
                        PendingAction::ResumeLoop(item.id.clone())
                    }
                    status => {
                        warn!("Cannot toggle status '{}' - no action taken", status);
                        return;
                    }
                };
                self.state.pending_action = Some(action.clone());
                info!("pending_action set to {:?}", action);
            } else {
                warn!("Execution '{}' not found in executions list", target_id);
            }
        }
    }

    /// Handle show logs in Describe view - navigates to Logs view for the execution
    fn handle_show_logs_describe(&mut self) {
        debug!("App::handle_show_logs_describe: called");
        if let View::Describe { ref target_id, .. } = self.state.current_view {
            debug!(%target_id, "App::handle_show_logs_describe: pushing Logs view");
            let id = target_id.clone();
            self.state.push_view(View::Logs { target_id: id });
        } else {
            debug!("App::handle_show_logs_describe: not in Describe view");
        }
    }

    /// Handle key in filter mode
    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_filter_key: called");
        match key.code {
            KeyCode::Esc => {
                debug!("App::handle_filter_key: Esc - cancel filter");
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                debug!("App::handle_filter_key: Enter - apply filter");
                // Apply the filter
                if let InteractionMode::Filter(text) = &self.state.interaction_mode {
                    self.state.filter_text = text.clone();
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Backspace => {
                debug!("App::handle_filter_key: Backspace");
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                debug!(%c, "App::handle_filter_key: Char");
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.push(c);
                }
            }
            _ => {
                debug!("App::handle_filter_key: unhandled key");
            }
        }

        false
    }

    /// Handle key in task input mode
    fn handle_task_input_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_task_input_key: called");
        match key.code {
            KeyCode::Esc => {
                debug!("App::handle_task_input_key: Esc - cancel task input");
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                debug!("App::handle_task_input_key: Enter - submit task");
                // Submit the task (store it for runner to process)
                if let InteractionMode::TaskInput(task) = &self.state.interaction_mode
                    && !task.trim().is_empty()
                {
                    debug!(%task, "App::handle_task_input_key: setting pending task");
                    self.state.pending_task = Some(task.clone());
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Backspace => {
                debug!("App::handle_task_input_key: Backspace");
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                debug!(%c, "App::handle_task_input_key: Char");
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.push(c);
                }
            }
            _ => {
                debug!("App::handle_task_input_key: unhandled key");
            }
        }

        false
    }

    /// Handle key in REPL input mode
    fn handle_repl_input_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_repl_input_key: called");
        match key.code {
            KeyCode::Esc => {
                debug!("App::handle_repl_input_key: Esc - cancel input");
                // Clear input and return to normal mode
                self.state.repl_input.clear();
                self.state.repl_cursor_pos = 0;
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                debug!("App::handle_repl_input_key: Enter - submit input");
                // Submit the input for processing
                let input = std::mem::take(&mut self.state.repl_input);
                self.state.repl_cursor_pos = 0;
                if !input.trim().is_empty() {
                    // Handle slash commands
                    if input.starts_with('/') {
                        debug!("App::handle_repl_input_key: handling slash command");
                        self.handle_repl_slash_command(&input);
                    } else {
                        debug!("App::handle_repl_input_key: queuing for LLM processing");
                        // Queue for LLM processing
                        self.state.pending_repl_submit = Some(input);
                    }
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Backspace => {
                debug!("App::handle_repl_input_key: Backspace");
                if self.state.repl_cursor_pos > 0 {
                    // Find the previous character boundary
                    let new_pos = self.prev_char_boundary(self.state.repl_cursor_pos);
                    self.state.repl_input.drain(new_pos..self.state.repl_cursor_pos);
                    self.state.repl_cursor_pos = new_pos;
                }
                // If input is empty, return to normal mode
                if self.state.repl_input.is_empty() {
                    debug!("App::handle_repl_input_key: input empty, returning to normal mode");
                    self.state.interaction_mode = InteractionMode::Normal;
                }
            }
            KeyCode::Delete => {
                debug!("App::handle_repl_input_key: Delete");
                if self.state.repl_cursor_pos < self.state.repl_input.len() {
                    // Find the next character boundary
                    let end_pos = self.next_char_boundary(self.state.repl_cursor_pos);
                    self.state.repl_input.drain(self.state.repl_cursor_pos..end_pos);
                }
            }
            KeyCode::Char(c) => {
                debug!(%c, "App::handle_repl_input_key: Char");
                self.state.repl_input.insert(self.state.repl_cursor_pos, c);
                self.state.repl_cursor_pos += c.len_utf8();
            }
            // Tab cycles through views (exit input mode first)
            KeyCode::Tab => {
                debug!("App::handle_repl_input_key: Tab - next view");
                self.state.interaction_mode = InteractionMode::Normal;
                self.navigate_next_top_level_view();
            }
            KeyCode::BackTab => {
                debug!("App::handle_repl_input_key: BackTab - prev view");
                self.state.interaction_mode = InteractionMode::Normal;
                self.navigate_prev_top_level_view();
            }
            // Cursor movement
            KeyCode::Left => {
                debug!("App::handle_repl_input_key: Left - cursor left");
                if self.state.repl_cursor_pos > 0 {
                    self.state.repl_cursor_pos = self.prev_char_boundary(self.state.repl_cursor_pos);
                }
            }
            KeyCode::Right => {
                debug!("App::handle_repl_input_key: Right - cursor right");
                if self.state.repl_cursor_pos < self.state.repl_input.len() {
                    self.state.repl_cursor_pos = self.next_char_boundary(self.state.repl_cursor_pos);
                }
            }
            KeyCode::Home => {
                debug!("App::handle_repl_input_key: Home - cursor to start");
                self.state.repl_cursor_pos = 0;
            }
            KeyCode::End => {
                debug!("App::handle_repl_input_key: End - cursor to end");
                self.state.repl_cursor_pos = self.state.repl_input.len();
            }
            _ => {
                debug!("App::handle_repl_input_key: unhandled key");
            }
        }

        false
    }

    /// Find the previous character boundary in the input
    fn prev_char_boundary(&self, pos: usize) -> usize {
        debug!(%pos, "App::prev_char_boundary: called");
        let input = &self.state.repl_input;
        let mut new_pos = pos.saturating_sub(1);
        while new_pos > 0 && !input.is_char_boundary(new_pos) {
            new_pos -= 1;
        }
        debug!(%new_pos, "App::prev_char_boundary: result");
        new_pos
    }

    /// Find the next character boundary in the input
    fn next_char_boundary(&self, pos: usize) -> usize {
        debug!(%pos, "App::next_char_boundary: called");
        let input = &self.state.repl_input;
        let mut new_pos = pos + 1;
        while new_pos < input.len() && !input.is_char_boundary(new_pos) {
            new_pos += 1;
        }
        let result = new_pos.min(input.len());
        debug!(%result, "App::next_char_boundary: result");
        result
    }

    /// Handle /create command to generate a plan from the conversation
    fn handle_create_plan_command(&mut self) {
        debug!("App::handle_create_plan_command: called");
        // Check we're in Plan mode
        if self.state.repl_mode != ReplMode::Plan {
            debug!("App::handle_create_plan_command: not in Plan mode");
            self.state.set_error("Switch to Plan mode first (press P)");
            return;
        }

        // Check we have conversation history
        if self.state.repl_history.is_empty() {
            debug!("App::handle_create_plan_command: no conversation history");
            self.state
                .set_error("No conversation to create plan from. Describe your requirements first.");
            return;
        }

        // Check we're not already creating a plan
        if self.state.pending_plan_create.is_some() || self.state.plan_creating {
            debug!("App::handle_create_plan_command: plan creation already in progress");
            self.state.set_error("Plan creation already in progress");
            return;
        }

        debug!("App::handle_create_plan_command: queueing plan creation");
        // Add the /create command to the conversation history so it's visible
        self.state.repl_history.push(ReplMessage::user("/create".to_string()));

        // Queue the plan creation request
        self.state.pending_plan_create = Some(PlanCreateRequest {
            messages: self.state.repl_history.clone(),
        });
    }

    /// Handle REPL slash commands
    fn handle_repl_slash_command(&mut self, input: &str) {
        debug!(%input, "App::handle_repl_slash_command: called");
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");

        match cmd {
            "/help" | "/h" => {
                debug!("App::handle_repl_slash_command: help command");
                self.state.interaction_mode = InteractionMode::Help;
            }
            "/quit" | "/q" | "/exit" => {
                debug!("App::handle_repl_slash_command: quit command");
                if self.state.executions_active > 0 {
                    debug!("App::handle_repl_slash_command: showing quit confirm");
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
                } else {
                    debug!("App::handle_repl_slash_command: quitting");
                    self.state.should_quit = true;
                }
            }
            "/clear" | "/c" => {
                debug!("App::handle_repl_slash_command: clear command");
                self.state.repl_history.clear();
                self.state.repl_response_buffer.clear();
                self.state.repl_scroll = None; // Reset to auto-scroll
            }
            "/create" => {
                debug!("App::handle_repl_slash_command: create command");
                self.handle_create_plan_command();
            }
            "/executions" | "/exec" => {
                debug!("App::handle_repl_slash_command: executions command");
                self.state.current_view = View::Executions;
                self.state.view_stack.clear();
            }
            "/records" | "/rec" => {
                debug!("App::handle_repl_slash_command: records command");
                self.state.current_view = View::Records {
                    type_filter: None,
                    parent_filter: None,
                };
                self.state.view_stack.clear();
            }
            _ => {
                debug!(%cmd, "App::handle_repl_slash_command: unknown command");
                self.state.set_error(format!("Unknown command: {}", cmd));
            }
        }
    }

    /// Handle key in command mode
    fn handle_command_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_command_key: called");
        match key.code {
            KeyCode::Esc => {
                debug!("App::handle_command_key: Esc - cancel command");
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                debug!("App::handle_command_key: Enter - execute command");
                // Execute the command
                if let InteractionMode::Command(cmd) = &self.state.interaction_mode {
                    self.execute_command(cmd.clone());
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Backspace => {
                debug!("App::handle_command_key: Backspace");
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.pop();
                }
            }
            KeyCode::Char(c) => {
                debug!(%c, "App::handle_command_key: Char");
                if let Some(buf) = self.state.interaction_mode.input_buffer_mut() {
                    buf.push(c);
                }
            }
            _ => {
                debug!("App::handle_command_key: unhandled key");
            }
        }

        false
    }

    /// Handle key in confirm dialog
    fn handle_confirm_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_confirm_key: called");
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                debug!("App::handle_confirm_key: cancel confirm");
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Enter => {
                debug!("App::handle_confirm_key: Enter - process confirm");
                if let InteractionMode::Confirm(dialog) = &self.state.interaction_mode
                    && dialog.selected_button
                {
                    debug!("App::handle_confirm_key: user confirmed");
                    // User confirmed - queue action for runner to execute
                    match &dialog.action {
                        ConfirmAction::Quit => {
                            debug!("App::handle_confirm_key: quit confirmed");
                            self.state.should_quit = true;
                        }
                        ConfirmAction::CancelLoop(id) => {
                            debug!(%id, "App::handle_confirm_key: cancel loop confirmed");
                            self.state.pending_action = Some(PendingAction::CancelLoop(id.clone()));
                        }
                        ConfirmAction::PauseLoop(id) => {
                            debug!(%id, "App::handle_confirm_key: pause loop confirmed");
                            self.state.pending_action = Some(PendingAction::PauseLoop(id.clone()));
                        }
                        ConfirmAction::ResumeLoop(id) => {
                            debug!(%id, "App::handle_confirm_key: resume loop confirmed");
                            self.state.pending_action = Some(PendingAction::ResumeLoop(id.clone()));
                        }
                        ConfirmAction::DeleteExecution(id) => {
                            debug!(%id, "App::handle_confirm_key: delete execution confirmed");
                            self.state.pending_action = Some(PendingAction::DeleteExecution(id.clone()));
                        }
                        ConfirmAction::ActivateDraft(id) => {
                            debug!(%id, "App::handle_confirm_key: activate draft confirmed");
                            self.state.pending_action = Some(PendingAction::ActivateDraft(id.clone()));
                        }
                    }
                } else {
                    debug!("App::handle_confirm_key: user did not confirm");
                }
                self.state.interaction_mode = InteractionMode::Normal;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('y') | KeyCode::Char('Y') => {
                debug!("App::handle_confirm_key: toggle button selection");
                // Toggle button selection
                if let InteractionMode::Confirm(dialog) = &mut self.state.interaction_mode {
                    if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
                        debug!("App::handle_confirm_key: yes selected");
                        dialog.selected_button = true;
                    } else {
                        debug!("App::handle_confirm_key: toggle selection");
                        dialog.selected_button = !dialog.selected_button;
                    }
                }
            }
            _ => {
                debug!("App::handle_confirm_key: unhandled key");
            }
        }

        false
    }

    /// Handle key in help mode
    fn handle_help_key(&mut self, key: KeyEvent) -> bool {
        debug!(?key, "App::handle_help_key: called");
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                debug!("App::handle_help_key: closing help");
                self.state.interaction_mode = InteractionMode::Normal;
            }
            _ => {
                debug!("App::handle_help_key: unhandled key");
            }
        }

        false
    }

    /// Execute a command from command mode
    fn execute_command(&mut self, cmd: String) {
        debug!(%cmd, "App::execute_command: called");
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            debug!("App::execute_command: empty command");
            return;
        }

        let command = parts[0];

        // Try view switching first (delegates to View::from_command which defines available views)
        if let Some(view) = View::from_command(command, &self.state.available_types) {
            debug!(?view, "App::execute_command: switching view");
            self.state.navigate_to(view);
            return;
        }

        // Handle other commands
        match command {
            // Create new task - enter task input mode
            "new" => {
                debug!("App::execute_command: new task command");
                self.state.navigate_to(View::Executions); // Switch to executions view
                self.state.interaction_mode = InteractionMode::TaskInput(String::new());
            }

            // Quit
            "quit" | "q" => {
                debug!("App::execute_command: quit command");
                if self.state.executions_active > 0 {
                    debug!("App::execute_command: showing quit confirm");
                    self.state.interaction_mode = InteractionMode::Confirm(ConfirmDialog::quit());
                } else {
                    debug!("App::execute_command: quitting");
                    self.state.should_quit = true;
                }
            }

            // Help
            "help" => {
                debug!("App::execute_command: help command");
                self.state.interaction_mode = InteractionMode::Help;
            }

            _ => {
                debug!(%command, "App::execute_command: unknown command");
                self.state.set_error(format!("Unknown command: {}", command));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::ExecutionItem;

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
        // Switch to Executions view since '/' in REPL view is treated as input
        app.state_mut().current_view = View::Executions;

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
        // Switch to Executions view since ':' in REPL view is treated as input
        app.state_mut().current_view = View::Executions;

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

        // :loops now switches to View::Loops (tree view)
        app.execute_command("loops".to_string());
        assert!(matches!(app.state().current_view, View::Loops));

        // :executions still switches to legacy flat view
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

    // === Helper to create test ExecutionItem ===
    fn make_execution_item(id: &str, status: &str, parent: Option<&str>) -> ExecutionItem {
        ExecutionItem {
            id: id.to_string(),
            name: format!("Test {}", id),
            loop_type: "plan".to_string(),
            iteration: "1/10".to_string(),
            status: status.to_string(),
            duration: "0:00".to_string(),
            parent_id: parent.map(|s| s.to_string()),
            progress: String::new(),
            artifact_id: None,
            artifact_file: None,
            artifact_status: None,
        }
    }

    // === POSITIVE TESTS: handle_start_draft ===

    #[test]
    fn test_start_draft_shows_confirm_dialog_in_loops_view() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        // Add a draft execution to the tree
        let items = vec![make_execution_item("draft-1", "draft", None)];
        app.state_mut().loops_tree.build_from_items(items);

        // Verify selection
        assert_eq!(app.state().loops_tree.selected_id(), Some(&"draft-1".to_string()));

        // Press 's' to start draft - directly sets pending action (no confirm)
        let key = KeyEvent::from(KeyCode::Char('s'));
        app.handle_key(key);

        // Should directly set pending action
        assert!(matches!(app.state().pending_action, Some(PendingAction::ActivateDraft(ref id)) if id == "draft-1"));
    }

    #[test]
    fn test_start_draft_sets_pending_action_in_executions_view() {
        let mut app = App::new();
        app.state_mut().current_view = View::Executions;

        // Add a draft execution
        let items = vec![make_execution_item("draft-2", "draft", None)];
        app.state_mut().executions = items;

        // Press 's' to start draft
        let key = KeyEvent::from(KeyCode::Char('s'));
        app.handle_key(key);

        // Should directly set pending action
        assert!(matches!(app.state().pending_action, Some(PendingAction::ActivateDraft(ref id)) if id == "draft-2"));
    }

    #[test]
    fn test_start_draft_with_nested_tree_selection() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        // Add a tree with parent and child draft
        let items = vec![
            make_execution_item("parent-1", "running", None),
            make_execution_item("child-draft", "draft", Some("parent-1")),
        ];
        app.state_mut().loops_tree.build_from_items(items);

        // Navigate to child
        app.state_mut().loops_tree.select_next();
        assert_eq!(app.state().loops_tree.selected_id(), Some(&"child-draft".to_string()));

        // Press 's' to start draft
        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Should directly set pending action for child
        assert!(
            matches!(app.state().pending_action, Some(PendingAction::ActivateDraft(ref id)) if id == "child-draft")
        );
    }

    // === TESTS: toggle state for different statuses ===

    #[test]
    fn test_toggle_state_sets_pause_for_running() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        let items = vec![make_execution_item("running-1", "running", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Should directly set pause pending action
        assert!(matches!(app.state().pending_action, Some(PendingAction::PauseLoop(ref id)) if id == "running-1"));
    }

    #[test]
    fn test_toggle_state_does_nothing_for_complete() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        let items = vec![make_execution_item("complete-1", "complete", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Complete status cannot be toggled - no pending action set
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_sets_resume_for_paused() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        let items = vec![make_execution_item("paused-1", "paused", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Should directly set resume pending action
        assert!(matches!(app.state().pending_action, Some(PendingAction::ResumeLoop(ref id)) if id == "paused-1"));
    }

    #[test]
    fn test_toggle_state_does_nothing_for_failed() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        let items = vec![make_execution_item("failed-1", "failed", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Failed status cannot be toggled - no pending action set
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_does_nothing_for_pending() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        let items = vec![make_execution_item("pending-1", "pending", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Pending cannot be toggled - waiting for daemon to pick up
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_does_nothing_when_tree_empty() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        // Empty tree, no items
        app.state_mut().loops_tree.build_from_items(vec![]);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // No item selected - no pending action set
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_does_nothing_in_repl_view() {
        let mut app = App::new();
        app.state_mut().current_view = View::Repl;

        // Even with draft in tree, 's' in Repl view should not trigger toggle
        let items = vec![make_execution_item("draft-5", "draft", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // In Repl view, 's' goes to input, not toggle state
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_does_nothing_in_records_view() {
        let mut app = App::new();
        app.state_mut().current_view = View::Records {
            type_filter: None,
            parent_filter: None,
        };

        let items = vec![make_execution_item("draft-6", "draft", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Records view doesn't support toggle
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_does_nothing_in_describe_view() {
        let mut app = App::new();
        app.state_mut().current_view = View::Describe {
            target_id: "some-id".to_string(),
            target_type: "plan".to_string(),
        };

        let items = vec![make_execution_item("draft-7", "draft", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Describe view doesn't support toggle
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_does_nothing_in_logs_view() {
        let mut app = App::new();
        app.state_mut().current_view = View::Logs {
            target_id: "some-id".to_string(),
        };

        let items = vec![make_execution_item("draft-8", "draft", None)];
        app.state_mut().loops_tree.build_from_items(items);

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Logs view doesn't support toggle
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_executions_view_empty_list() {
        let mut app = App::new();
        app.state_mut().current_view = View::Executions;
        app.state_mut().executions = vec![]; // Empty

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // No item selected - no pending action set
        assert!(app.state().pending_action.is_none());
    }

    #[test]
    fn test_toggle_state_executions_view_running_sets_pause() {
        let mut app = App::new();
        app.state_mut().current_view = View::Executions;
        app.state_mut().executions = vec![make_execution_item("running-2", "running", None)];

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        // Running item should directly set pause pending action
        assert!(matches!(app.state().pending_action, Some(PendingAction::PauseLoop(ref id)) if id == "running-2"));
    }

    // === TESTS: Tree selection persists correctly ===

    #[test]
    fn test_tree_selection_persists_after_rebuild() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        // Build tree with multiple items
        let items = vec![
            make_execution_item("item-1", "draft", None),
            make_execution_item("item-2", "draft", None),
        ];
        app.state_mut().loops_tree.build_from_items(items.clone());

        // Select second item
        app.state_mut().loops_tree.select_next();
        assert_eq!(app.state().loops_tree.selected_id(), Some(&"item-2".to_string()));

        // Rebuild tree (simulating refresh)
        app.state_mut().loops_tree.build_from_items(items);

        // Selection should persist
        assert_eq!(app.state().loops_tree.selected_id(), Some(&"item-2".to_string()));
    }

    #[test]
    fn test_tree_selection_resets_when_selected_item_removed() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        // Build tree
        let items = vec![
            make_execution_item("item-1", "draft", None),
            make_execution_item("item-2", "draft", None),
        ];
        app.state_mut().loops_tree.build_from_items(items);

        // Select second item
        app.state_mut().loops_tree.select_next();
        assert_eq!(app.state().loops_tree.selected_id(), Some(&"item-2".to_string()));

        // Rebuild with item-2 removed
        let new_items = vec![make_execution_item("item-1", "draft", None)];
        app.state_mut().loops_tree.build_from_items(new_items);

        // Selection should reset to first item
        assert_eq!(app.state().loops_tree.selected_id(), Some(&"item-1".to_string()));
    }

    #[test]
    fn test_tree_selection_on_empty_becomes_none() {
        let mut app = App::new();
        app.state_mut().current_view = View::Loops;

        // Build tree
        let items = vec![make_execution_item("item-1", "draft", None)];
        app.state_mut().loops_tree.build_from_items(items);
        assert!(app.state().loops_tree.selected_id().is_some());

        // Rebuild with empty
        app.state_mut().loops_tree.build_from_items(vec![]);

        // Selection should be None
        assert!(app.state().loops_tree.selected_id().is_none());
    }
}
