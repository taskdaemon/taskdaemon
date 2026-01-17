//! TUI application state
//!
//! Pure data structures for the TUI. No rendering logic here.
//! Follows the k9s-style resource navigation pattern.
//!
//! Views are dynamic based on loaded loop types from YAML configuration.

/// Which view is currently displayed (k9s-style)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum View {
    /// Interactive REPL for AI-assisted coding (default view)
    #[default]
    Repl,
    /// All running executions (`:loops` or `:executions`)
    Executions,
    /// Loop records filtered by type (`:records` or `:<type>` e.g., `:plan`)
    Records {
        /// Filter to specific loop type (None = all records)
        type_filter: Option<String>,
        /// Filter to specific parent ID
        parent_filter: Option<String>,
    },
    /// Logs view for a specific resource (`l` key)
    Logs { target_id: String },
    /// Describe view with full details (`d` key)
    Describe {
        target_id: String,
        /// The loop type of the target (for context)
        target_type: String,
    },
}

/// Top-level views for arrow key navigation (in order)
pub const TOP_LEVEL_VIEWS: [View; 3] = [
    View::Repl,
    View::Executions,
    View::Records {
        type_filter: None,
        parent_filter: None,
    },
];

/// Get the index of a view in the top-level views list
pub fn top_level_view_index(view: &View) -> usize {
    match view {
        View::Repl => 0,
        View::Executions => 1,
        View::Records { .. } => 2,
        _ => 0, // Default to Repl for non-top-level views
    }
}

impl View {
    /// Get the display name for the header
    pub fn display_name(&self) -> String {
        match self {
            Self::Repl => "REPL".to_string(),
            Self::Records {
                type_filter: Some(t), ..
            } => format!("Records ({})", t),
            Self::Records { type_filter: None, .. } => "Records".to_string(),
            Self::Executions => "Executions".to_string(),
            Self::Logs { .. } => "Logs".to_string(),
            Self::Describe { .. } => "Describe".to_string(),
        }
    }

    /// Parse a command name to a View
    ///
    /// Built-in commands:
    /// - `repl` - show the interactive REPL
    /// - `records` or `all` - show all Loop records
    /// - `loops` or `executions` - show all LoopExecution records
    ///
    /// Dynamic commands (based on loaded loop types):
    /// - Any loaded type name (e.g., `plan`, `spec`) filters Records by that type
    pub fn from_command(cmd: &str, available_types: &[String]) -> Option<Self> {
        match cmd {
            // REPL view
            "repl" => Some(Self::Repl),
            // All records
            "records" | "all" => Some(Self::Records {
                type_filter: None,
                parent_filter: None,
            }),
            // All executions
            "loops" | "executions" => Some(Self::Executions),
            // Check if it's a known loop type
            _ => {
                if available_types.iter().any(|t| t == cmd) {
                    Some(Self::Records {
                        type_filter: Some(cmd.to_string()),
                        parent_filter: None,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Check if this is a list view (can navigate with j/k)
    pub fn is_list_view(&self) -> bool {
        matches!(self, Self::Records { .. } | Self::Executions)
    }

    /// Check if this is a top-level view (can navigate with left/right)
    pub fn is_top_level(&self) -> bool {
        matches!(self, Self::Repl | Self::Executions | Self::Records { .. })
    }
}

/// Interaction mode (modal)
#[derive(Debug, Clone, Default)]
pub enum InteractionMode {
    /// Normal navigation mode
    #[default]
    Normal,
    /// Search/filter mode (/ key)
    Filter(String),
    /// Command mode (: key)
    Command(String),
    /// Task input mode (n key)
    TaskInput(String),
    /// REPL input mode (typing in REPL view)
    ReplInput,
    /// Confirmation dialog
    Confirm(ConfirmDialog),
    /// Help overlay
    Help,
}

impl InteractionMode {
    /// Check if in filter mode
    pub fn is_filter(&self) -> bool {
        matches!(self, Self::Filter(_))
    }

    /// Check if in command mode
    pub fn is_command(&self) -> bool {
        matches!(self, Self::Command(_))
    }

    /// Get the input buffer if in an input mode
    pub fn input_buffer(&self) -> Option<&str> {
        match self {
            Self::Filter(s) | Self::Command(s) | Self::TaskInput(s) => Some(s),
            _ => None,
        }
    }

    /// Get mutable input buffer
    pub fn input_buffer_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Filter(s) | Self::Command(s) | Self::TaskInput(s) => Some(s),
            _ => None,
        }
    }
}

/// Confirmation dialog for dangerous actions
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub message: String,
    pub action: ConfirmAction,
    pub selected_button: bool, // false = No, true = Yes
}

impl ConfirmDialog {
    pub fn new(action: ConfirmAction, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            action,
            selected_button: false,
        }
    }

    pub fn quit() -> Self {
        Self::new(
            ConfirmAction::Quit,
            "There are running loops. Are you sure you want to quit?",
        )
    }

    pub fn cancel_loop(id: String, name: &str) -> Self {
        Self::new(ConfirmAction::CancelLoop(id), format!("Cancel {}?", name))
    }

    pub fn pause_loop(id: String, name: &str) -> Self {
        Self::new(ConfirmAction::PauseLoop(id), format!("Pause {}?", name))
    }

    pub fn delete_execution(id: String, name: &str) -> Self {
        Self::new(
            ConfirmAction::DeleteExecution(id),
            format!("Delete {}? This removes it from view.", name),
        )
    }
}

/// Action to perform on confirm
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Quit,
    CancelLoop(String),
    PauseLoop(String),
    ResumeLoop(String),
    DeleteExecution(String),
    StartDraft(String),
}

/// Action pending execution by the runner
#[derive(Debug, Clone)]
pub enum PendingAction {
    CancelLoop(String),
    PauseLoop(String),
    ResumeLoop(String),
    DeleteExecution(String),
    StartDraft(String),
}

/// Request to create a plan from the current conversation
#[derive(Debug, Clone)]
pub struct PlanCreateRequest {
    /// The conversation messages to summarize
    pub messages: Vec<ReplMessage>,
}

/// REPL mode (Chat vs Plan)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplMode {
    /// Interactive chat mode (default)
    #[default]
    Chat,
    /// Plan mode for structured planning
    Plan,
}

/// REPL message role
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplRole {
    User,
    Assistant,
    ToolResult { tool_name: String },
    Error,
}

/// REPL message for display
#[derive(Debug, Clone)]
pub struct ReplMessage {
    pub role: ReplRole,
    pub content: String,
    pub timestamp: i64,
}

impl ReplMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ReplRole::User,
            content: content.into(),
            timestamp: taskstore::now_ms(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ReplRole::Assistant,
            content: content.into(),
            timestamp: taskstore::now_ms(),
        }
    }

    pub fn tool_result(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ReplRole::ToolResult {
                tool_name: tool_name.into(),
            },
            content: content.into(),
            timestamp: taskstore::now_ms(),
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            role: ReplRole::Error,
            content: content.into(),
            timestamp: taskstore::now_ms(),
        }
    }
}

/// Selection state for list views
#[derive(Debug, Default, Clone)]
pub struct SelectionState {
    pub selected_index: usize,
    pub scroll_offset: usize,
}

impl SelectionState {
    pub fn select_next(&mut self, max_items: usize) {
        if max_items > 0 && self.selected_index < max_items - 1 {
            self.selected_index += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    pub fn select_last(&mut self, max_items: usize) {
        if max_items > 0 {
            self.selected_index = max_items - 1;
        }
    }

    /// Ensure selection is within bounds
    pub fn clamp(&mut self, max_items: usize) {
        if max_items == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= max_items {
            self.selected_index = max_items - 1;
        }
    }
}

/// Main TUI application state
#[derive(Debug)]
pub struct AppState {
    /// Current view
    pub current_view: View,
    /// View history for back navigation
    pub view_stack: Vec<View>,
    /// Current interaction mode
    pub interaction_mode: InteractionMode,
    /// Current filter text (for / filtering)
    pub filter_text: String,
    /// Should the app quit
    pub should_quit: bool,
    /// Last error message
    pub error_message: Option<String>,

    // === Cached data for display ===
    /// Loop records (filtered by current view)
    pub records: Vec<RecordItem>,
    /// Loop executions
    pub executions: Vec<ExecutionItem>,
    /// Log entries for current target
    pub logs: Vec<LogEntry>,
    /// Describe data for current target
    pub describe_data: Option<DescribeData>,

    // === Selection state per view ===
    pub records_selection: SelectionState,
    pub executions_selection: SelectionState,

    // === Metrics ===
    pub total_records: usize,
    pub executions_active: usize,
    pub executions_complete: usize,
    pub executions_failed: usize,

    // === Available loop types (from config) ===
    pub available_types: Vec<String>,

    // === Logs view state ===
    pub logs_follow: bool,
    pub logs_scroll: usize,

    // === Pending actions ===
    pub pending_task: Option<String>,
    pub pending_action: Option<PendingAction>,

    // === Last data refresh ===
    pub last_refresh: i64,

    // === REPL state ===
    /// Current REPL mode (Chat or Plan)
    pub repl_mode: ReplMode,
    /// Conversation history for display
    pub repl_history: Vec<ReplMessage>,
    /// Current input buffer
    pub repl_input: String,
    /// Is the LLM currently streaming a response?
    pub repl_streaming: bool,
    /// Accumulating stream response (for incremental display)
    pub repl_response_buffer: String,
    /// Queued input for async processing
    pub pending_repl_submit: Option<String>,
    /// Scroll offset for REPL history view
    pub repl_scroll: usize,
    /// Pending plan creation request
    pub pending_plan_create: Option<PlanCreateRequest>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_view: View::default(),
            view_stack: Vec::new(),
            interaction_mode: InteractionMode::default(),
            filter_text: String::new(),
            should_quit: false,
            error_message: None,
            records: Vec::new(),
            executions: Vec::new(),
            logs: Vec::new(),
            describe_data: None,
            records_selection: SelectionState::default(),
            executions_selection: SelectionState::default(),
            total_records: 0,
            executions_active: 0,
            executions_complete: 0,
            executions_failed: 0,
            available_types: Vec::new(),
            logs_follow: true,
            logs_scroll: 0,
            pending_task: None,
            pending_action: None,
            last_refresh: 0,
            // REPL state
            repl_mode: ReplMode::default(),
            repl_history: Vec::new(),
            repl_input: String::new(),
            repl_streaming: false,
            repl_response_buffer: String::new(),
            pending_repl_submit: None,
            repl_scroll: 0,
            pending_plan_create: None,
        }
    }
}

impl AppState {
    /// Create new AppState
    pub fn new() -> Self {
        Self::default()
    }

    /// Navigate to a new view, pushing current to stack
    pub fn navigate_to(&mut self, view: View) {
        self.view_stack.push(self.current_view.clone());
        self.current_view = view;
        self.reset_selection();
        self.filter_text.clear();
    }

    /// Push a view to the stack and switch to it
    pub fn push_view(&mut self, view: View) {
        self.navigate_to(view);
    }

    /// Go back to previous view
    pub fn pop_view(&mut self) -> bool {
        if let Some(prev_view) = self.view_stack.pop() {
            self.current_view = prev_view;
            self.reset_selection();
            self.filter_text.clear();
            true
        } else {
            false
        }
    }

    /// Reset selection state for current view
    fn reset_selection(&mut self) {
        match &self.current_view {
            View::Records { .. } => self.records_selection = SelectionState::default(),
            View::Executions => self.executions_selection = SelectionState::default(),
            _ => {}
        }
    }

    /// Get mutable selection state for current view
    pub fn current_selection_mut(&mut self) -> Option<&mut SelectionState> {
        match &self.current_view {
            View::Records { .. } => Some(&mut self.records_selection),
            View::Executions => Some(&mut self.executions_selection),
            _ => None,
        }
    }

    /// Get item count for current view
    pub fn current_item_count(&self) -> usize {
        match &self.current_view {
            View::Repl => self.repl_history.len(),
            View::Records { .. } => self.filtered_records().len(),
            View::Executions => self.filtered_executions().len(),
            View::Logs { .. } => self.logs.len(),
            View::Describe { .. } => 0,
        }
    }

    /// Set an error message
    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.error_message = Some(msg.into());
    }

    /// Clear error message
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Get the ID of the currently selected item
    pub fn selected_item_id(&self) -> Option<String> {
        match &self.current_view {
            View::Records { .. } => {
                let filtered = self.filtered_records();
                filtered
                    .get(self.records_selection.selected_index)
                    .map(|r| r.id.clone())
            }
            View::Executions => {
                let filtered = self.filtered_executions();
                filtered
                    .get(self.executions_selection.selected_index)
                    .map(|e| e.id.clone())
            }
            _ => None,
        }
    }

    /// Get the name/title of the currently selected item
    pub fn selected_item_name(&self) -> Option<String> {
        match &self.current_view {
            View::Records { .. } => {
                let filtered = self.filtered_records();
                filtered
                    .get(self.records_selection.selected_index)
                    .map(|r| r.title.clone())
            }
            View::Executions => {
                let filtered = self.filtered_executions();
                filtered
                    .get(self.executions_selection.selected_index)
                    .map(|e| e.name.clone())
            }
            _ => None,
        }
    }

    /// Get the type of the currently selected item
    pub fn selected_item_type(&self) -> Option<String> {
        match &self.current_view {
            View::Records { .. } => {
                let filtered = self.filtered_records();
                filtered
                    .get(self.records_selection.selected_index)
                    .map(|r| r.loop_type.clone())
            }
            View::Executions => {
                let filtered = self.filtered_executions();
                filtered
                    .get(self.executions_selection.selected_index)
                    .map(|e| e.loop_type.clone())
            }
            _ => None,
        }
    }

    /// Get breadcrumb string for header
    pub fn breadcrumb(&self) -> String {
        self.current_view.display_name()
    }

    /// Tick - called on each frame update
    pub fn tick(&mut self) {
        // Update logs scroll if following
        if self.logs_follow && !self.logs.is_empty() {
            self.logs_scroll = self.logs.len().saturating_sub(1);
        }
    }

    /// Filter records by current filter text
    pub fn filtered_records(&self) -> Vec<&RecordItem> {
        if self.filter_text.is_empty() {
            self.records.iter().collect()
        } else {
            let filter = self.filter_text.to_lowercase();
            self.records
                .iter()
                .filter(|r| {
                    r.title.to_lowercase().contains(&filter)
                        || r.id.to_lowercase().contains(&filter)
                        || r.loop_type.to_lowercase().contains(&filter)
                })
                .collect()
        }
    }

    /// Filter executions by current filter text
    pub fn filtered_executions(&self) -> Vec<&ExecutionItem> {
        if self.filter_text.is_empty() {
            self.executions.iter().collect()
        } else {
            let filter = self.filter_text.to_lowercase();
            self.executions
                .iter()
                .filter(|e| {
                    e.name.to_lowercase().contains(&filter)
                        || e.id.to_lowercase().contains(&filter)
                        || e.loop_type.to_lowercase().contains(&filter)
                })
                .collect()
        }
    }
}

/// Cached Loop record item for display
#[derive(Debug, Clone)]
pub struct RecordItem {
    pub id: String,
    pub title: String,
    pub loop_type: String,
    pub status: String,
    pub parent_id: Option<String>,
    pub children_count: usize,
    pub phases_progress: String, // e.g., "2/4"
    pub created: String,         // e.g., "2m ago"
}

/// Cached loop execution item for display
#[derive(Debug, Clone)]
pub struct ExecutionItem {
    pub id: String,
    pub name: String,
    pub loop_type: String,
    pub iteration: String, // e.g., "3/10"
    pub status: String,
    pub duration: String, // e.g., "2:15"
    pub parent_id: Option<String>,
    pub progress: String, // last line of progress
}

/// Log entry for the logs view
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub iteration: u32,
    pub text: String,
    pub is_error: bool,
    pub is_stdout: bool,
}

/// Data for describe view
#[derive(Debug, Clone)]
pub struct DescribeData {
    pub id: String,
    pub loop_type: String,
    pub title: String,
    pub status: String,
    pub parent_id: Option<String>,
    pub created: String,
    pub updated: String,
    /// Key-value pairs for display
    pub fields: Vec<(String, String)>,
    /// Child records if any
    pub children: Vec<String>,
    /// Current execution info if running
    pub execution: Option<ExecutionInfo>,
}

/// Execution info for describe view
#[derive(Debug, Clone)]
pub struct ExecutionInfo {
    pub id: String,
    pub iteration: String,
    pub duration: String,
    pub progress: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_from_command_builtins() {
        let types = vec!["mytype".to_string()];
        assert!(matches!(
            View::from_command("records", &types),
            Some(View::Records { type_filter: None, .. })
        ));
        assert!(matches!(View::from_command("loops", &types), Some(View::Executions)));
        assert!(matches!(
            View::from_command("executions", &types),
            Some(View::Executions)
        ));
    }

    #[test]
    fn test_view_from_command_dynamic_type() {
        let types = vec!["plan".to_string(), "spec".to_string()];

        // Known types should work
        let view = View::from_command("plan", &types);
        assert!(matches!(view, Some(View::Records { type_filter: Some(t), .. }) if t == "plan"));

        let view = View::from_command("spec", &types);
        assert!(matches!(view, Some(View::Records { type_filter: Some(t), .. }) if t == "spec"));

        // Unknown types should return None
        assert!(View::from_command("unknown", &types).is_none());
    }

    #[test]
    fn test_selection_state_navigation() {
        let mut selection = SelectionState::default();

        // Move down
        selection.select_next(10);
        assert_eq!(selection.selected_index, 1);

        // Move up
        selection.select_prev();
        assert_eq!(selection.selected_index, 0);

        // Can't go below 0
        selection.select_prev();
        assert_eq!(selection.selected_index, 0);

        // Jump to last
        selection.select_last(10);
        assert_eq!(selection.selected_index, 9);

        // Can't go past end
        selection.select_next(10);
        assert_eq!(selection.selected_index, 9);
    }

    #[test]
    fn test_app_state_navigation() {
        let mut state = AppState::new();

        // Default view is now REPL
        assert!(matches!(state.current_view, View::Repl));

        // Navigate to records
        state.navigate_to(View::Records {
            type_filter: Some("mytype".to_string()),
            parent_filter: None,
        });
        assert!(matches!(state.current_view, View::Records { .. }));
        assert_eq!(state.view_stack.len(), 1);

        // Go back
        assert!(state.pop_view());
        assert!(matches!(state.current_view, View::Repl));
        assert_eq!(state.view_stack.len(), 0);

        // Can't go back further
        assert!(!state.pop_view());
    }

    #[test]
    fn test_view_from_command_repl() {
        let types = vec![];
        assert!(matches!(View::from_command("repl", &types), Some(View::Repl)));
    }

    #[test]
    fn test_top_level_view_index() {
        assert_eq!(top_level_view_index(&View::Repl), 0);
        assert_eq!(top_level_view_index(&View::Executions), 1);
        assert_eq!(
            top_level_view_index(&View::Records {
                type_filter: None,
                parent_filter: None
            }),
            2
        );
        // Non-top-level views should default to 0
        assert_eq!(
            top_level_view_index(&View::Logs {
                target_id: "test".to_string()
            }),
            0
        );
    }
}
