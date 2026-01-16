//! TUI application state
//!
//! Pure data structures for the TUI. No rendering logic here.
//! Follows the k9s-style resource navigation pattern.

use std::collections::HashMap;

/// Which resource type is currently displayed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ResourceView {
    #[default]
    Plans,
    Specs,
    Phases,
    Ralphs,
    // TaskStore analytics views
    Metrics,
    Costs,
    History,
    Dependencies,
}

impl ResourceView {
    /// Get the display name for this view
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Plans => "Plans",
            Self::Specs => "Specs",
            Self::Phases => "Phases",
            Self::Ralphs => "Ralphs",
            Self::Metrics => "Metrics",
            Self::Costs => "Costs",
            Self::History => "History",
            Self::Dependencies => "Dependencies",
        }
    }

    /// Get the command name for this view (used in command mode)
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::Plans => "plans",
            Self::Specs => "specs",
            Self::Phases => "phases",
            Self::Ralphs => "ralphs",
            Self::Metrics => "metrics",
            Self::Costs => "costs",
            Self::History => "history",
            Self::Dependencies => "deps",
        }
    }

    /// Parse a command name to a ResourceView
    pub fn from_command(cmd: &str) -> Option<Self> {
        match cmd {
            "plans" => Some(Self::Plans),
            "specs" => Some(Self::Specs),
            "phases" => Some(Self::Phases),
            "ralphs" | "loops" => Some(Self::Ralphs),
            "metrics" => Some(Self::Metrics),
            "costs" => Some(Self::Costs),
            "history" => Some(Self::History),
            "deps" | "dependencies" => Some(Self::Dependencies),
            _ => None,
        }
    }
}

/// Interaction mode (modal)
#[derive(Debug, Clone, Default)]
pub enum InteractionMode {
    /// Normal navigation mode
    #[default]
    Normal,
    /// Search/filter mode (/ key)
    Search(String),
    /// Command mode (: key)
    Command(String),
    /// Confirmation dialog
    Confirm(ConfirmDialog),
    /// Help overlay
    Help,
}

impl InteractionMode {
    /// Check if in search mode
    pub fn is_search(&self) -> bool {
        matches!(self, Self::Search(_))
    }

    /// Check if in command mode
    pub fn is_command(&self) -> bool {
        matches!(self, Self::Command(_))
    }

    /// Get the input buffer if in an input mode
    pub fn input_buffer(&self) -> Option<&str> {
        match self {
            Self::Search(s) | Self::Command(s) => Some(s),
            _ => None,
        }
    }

    /// Get mutable input buffer
    pub fn input_buffer_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Search(s) | Self::Command(s) => Some(s),
            _ => None,
        }
    }
}

/// Layout modes (from neuraphage)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutMode {
    #[default]
    Dashboard, // Sidebar + main view
    Split, // Two resources side-by-side
    Grid,  // 2x2 grid of resources
    Focus, // Full-screen single resource
}

impl LayoutMode {
    /// Cycle to next layout mode
    pub fn next(&self) -> Self {
        match self {
            Self::Dashboard => Self::Split,
            Self::Split => Self::Grid,
            Self::Grid => Self::Focus,
            Self::Focus => Self::Dashboard,
        }
    }
}

/// Confirm dialog for destructive actions
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub action: ConfirmAction,
    pub message: String,
    pub selected_button: bool, // false=No, true=Yes
}

impl ConfirmDialog {
    pub fn new(action: ConfirmAction, message: impl Into<String>) -> Self {
        Self {
            action,
            message: message.into(),
            selected_button: false, // Default to No (safe option)
        }
    }

    pub fn quit() -> Self {
        Self::new(ConfirmAction::Quit, "Are you sure you want to quit?")
    }
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    CancelLoop(String),
    PauseLoop(String),
    RestartLoop(String),
    DeletePlan(String),
    Quit,
}

/// Selection state per view
#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    /// Currently selected item index
    pub selected_index: usize,
    /// Scroll offset for the list
    pub scroll_offset: usize,
    /// Filter by parent ID (e.g., show specs for a specific plan)
    pub parent_filter: Option<String>,
}

impl SelectionState {
    /// Ensure selection is within bounds
    pub fn clamp(&mut self, max_items: usize) {
        if max_items == 0 {
            self.selected_index = 0;
            self.scroll_offset = 0;
        } else {
            self.selected_index = self.selected_index.min(max_items - 1);
            self.scroll_offset = self.scroll_offset.min(self.selected_index);
        }
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            }
        }
    }

    /// Move selection down
    pub fn select_next(&mut self, max_items: usize) {
        if max_items > 0 && self.selected_index + 1 < max_items {
            self.selected_index += 1;
        }
    }

    /// Jump to top
    pub fn select_first(&mut self) {
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Jump to bottom
    pub fn select_last(&mut self, max_items: usize) {
        if max_items > 0 {
            self.selected_index = max_items - 1;
        }
    }
}

/// Filter state
#[derive(Debug, Clone, Default)]
pub struct FilterState {
    /// Text filter (name contains)
    pub text: String,
    /// Status filter
    pub status_filter: Option<String>,
    /// Show only items needing attention
    pub attention_only: bool,
}

impl FilterState {
    /// Check if any filter is active
    pub fn is_active(&self) -> bool {
        !self.text.is_empty() || self.status_filter.is_some() || self.attention_only
    }

    /// Clear all filters
    pub fn clear(&mut self) {
        self.text.clear();
        self.status_filter = None;
        self.attention_only = false;
    }
}

/// Sort order for resource lists
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Status,
    Name,
    Activity,
    Priority,
}

impl SortOrder {
    /// Cycle to next sort order
    pub fn next(&self) -> Self {
        match self {
            Self::Status => Self::Name,
            Self::Name => Self::Activity,
            Self::Activity => Self::Priority,
            Self::Priority => Self::Status,
        }
    }

    /// Display name for this sort order
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Name => "Name",
            Self::Activity => "Activity",
            Self::Priority => "Priority",
        }
    }
}

/// Cached resource item for display
#[derive(Debug, Clone)]
pub struct ResourceItem {
    /// Unique identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Resource type string (for display)
    pub resource_type: String,
    /// Status string
    pub status: String,
    /// Parent resource ID (if any)
    pub parent_id: Option<String>,
    /// Iteration count (for loops)
    pub iteration: Option<u32>,
    /// Progress text
    pub progress: Option<String>,
    /// Last activity description
    pub last_activity: Option<String>,
    /// Whether this item needs attention (blocked, failed, etc.)
    pub needs_attention: bool,
    /// Reason for needing attention
    pub attention_reason: Option<String>,
}

impl ResourceItem {
    /// Truncate name to fit in column width
    pub fn truncated_name(&self, max_len: usize) -> String {
        if self.name.len() <= max_len {
            self.name.clone()
        } else if max_len > 3 {
            format!("{}...", &self.name[..max_len - 3])
        } else {
            self.name[..max_len].to_string()
        }
    }

    /// Get the status icon
    pub fn status_icon(&self) -> &'static str {
        match self.status.as_str() {
            "running" => "●",
            "pending" => "○",
            "blocked" => "?",
            "complete" => "✓",
            "failed" => "✗",
            "cancelled" | "stopped" => "⊘",
            "paused" => "◑",
            _ => " ",
        }
    }
}

/// Snapshot of view state for navigation history
#[derive(Debug, Clone)]
pub struct ViewSnapshot {
    pub view: ResourceView,
    pub selection: SelectionState,
}

/// Global metrics summary
#[derive(Debug, Clone, Default)]
pub struct GlobalMetrics {
    pub plans_total: usize,
    pub plans_active: usize,
    pub specs_total: usize,
    pub specs_running: usize,
    pub ralphs_active: usize,
    pub ralphs_complete: usize,
    pub ralphs_failed: usize,
    pub total_iterations: u64,
    pub total_api_calls: u64,
    pub total_cost_usd: f64,
}

/// TaskStore analytics: Metrics view data
#[derive(Debug, Clone, Default)]
pub struct MetricsViewData {
    pub by_loop_type: HashMap<String, TypeMetrics>,
    pub by_status: HashMap<String, usize>,
    pub iteration_histogram: Vec<(u32, usize)>, // (iteration_count, num_ralphs)
    pub success_rate: f64,
    pub avg_iterations_to_complete: f64,
}

/// Metrics for a specific loop type
#[derive(Debug, Clone, Default)]
pub struct TypeMetrics {
    pub total: usize,
    pub active: usize,
    pub complete: usize,
    pub failed: usize,
    pub avg_iterations: f64,
}

/// TaskStore analytics: Cost breakdown view
#[derive(Debug, Clone, Default)]
pub struct CostsViewData {
    pub total_cost_usd: f64,
    pub by_plan: Vec<(String, f64)>,      // (plan_id, cost)
    pub by_loop_type: Vec<(String, f64)>, // (loop_type, cost)
    pub by_day: Vec<(String, f64)>,       // (date, cost)
    pub token_breakdown: TokenBreakdown,
}

#[derive(Debug, Clone, Default)]
pub struct TokenBreakdown {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

/// TaskStore analytics: Iteration history timeline
#[derive(Debug, Clone, Default)]
pub struct HistoryViewData {
    pub events: Vec<HistoryEvent>,
    pub filter_type: Option<String>,
    pub filter_status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HistoryEvent {
    pub timestamp: i64,
    pub event_type: HistoryEventType,
    pub resource_id: String,
    pub resource_name: String,
    pub details: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryEventType {
    PlanCreated,
    PlanCompleted,
    SpecStarted,
    SpecCompleted,
    SpecFailed,
    RalphIteration,
    RalphCompleted,
    RalphFailed,
    MainBranchUpdated,
    MergeCompleted,
}

impl HistoryEventType {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::PlanCreated => "Plan Created",
            Self::PlanCompleted => "Plan Completed",
            Self::SpecStarted => "Spec Started",
            Self::SpecCompleted => "Spec Completed",
            Self::SpecFailed => "Spec Failed",
            Self::RalphIteration => "Ralph Iteration",
            Self::RalphCompleted => "Ralph Completed",
            Self::RalphFailed => "Ralph Failed",
            Self::MainBranchUpdated => "Main Updated",
            Self::MergeCompleted => "Merge Completed",
        }
    }
}

/// TaskStore analytics: Dependency graph visualization
#[derive(Debug, Clone, Default)]
pub struct DependencyGraphData {
    pub nodes: Vec<DependencyNode>,
    pub edges: Vec<DependencyEdge>,
    pub selected_node: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DependencyNode {
    pub id: String,
    pub name: String,
    pub node_type: String, // "plan", "spec", "phase", "ralph"
    pub status: String,
    pub x: f32, // Layout position
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub edge_type: String, // "parent", "depends_on", "spawned"
}

/// Main application state (pure data, no rendering)
#[derive(Debug)]
pub struct AppState {
    /// Current resource view
    pub current_view: ResourceView,

    /// Navigation history for Esc backtracking
    pub view_history: Vec<ViewSnapshot>,

    /// Current interaction mode
    pub interaction_mode: InteractionMode,

    /// Current layout mode
    pub layout_mode: LayoutMode,

    /// Cached resources by type
    pub plans: Vec<ResourceItem>,
    pub specs: Vec<ResourceItem>,
    pub phases: Vec<ResourceItem>,
    pub ralphs: Vec<ResourceItem>,

    /// TaskStore analytics data
    pub metrics_data: MetricsViewData,
    pub costs_data: CostsViewData,
    pub history_data: HistoryViewData,
    pub deps_graph: DependencyGraphData,

    /// Selection state per view
    pub selection: HashMap<ResourceView, SelectionState>,

    /// Global filter
    pub filter: FilterState,

    /// Sort order
    pub sort_order: SortOrder,

    /// Global metrics
    pub metrics: GlobalMetrics,

    /// Frame counter for animations
    pub frame_counter: u64,

    /// Should quit flag
    pub should_quit: bool,

    /// Error message to display (if any)
    pub error_message: Option<String>,

    /// Last refresh timestamp
    pub last_refresh: i64,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Create a new AppState with default values
    pub fn new() -> Self {
        let mut selection = HashMap::new();
        selection.insert(ResourceView::Plans, SelectionState::default());
        selection.insert(ResourceView::Specs, SelectionState::default());
        selection.insert(ResourceView::Phases, SelectionState::default());
        selection.insert(ResourceView::Ralphs, SelectionState::default());
        selection.insert(ResourceView::Metrics, SelectionState::default());
        selection.insert(ResourceView::Costs, SelectionState::default());
        selection.insert(ResourceView::History, SelectionState::default());
        selection.insert(ResourceView::Dependencies, SelectionState::default());

        Self {
            current_view: ResourceView::Plans,
            view_history: Vec::new(),
            interaction_mode: InteractionMode::Normal,
            layout_mode: LayoutMode::Dashboard,
            plans: Vec::new(),
            specs: Vec::new(),
            phases: Vec::new(),
            ralphs: Vec::new(),
            metrics_data: MetricsViewData::default(),
            costs_data: CostsViewData::default(),
            history_data: HistoryViewData::default(),
            deps_graph: DependencyGraphData::default(),
            selection,
            filter: FilterState::default(),
            sort_order: SortOrder::default(),
            metrics: GlobalMetrics::default(),
            frame_counter: 0,
            should_quit: false,
            error_message: None,
            last_refresh: 0,
        }
    }

    /// Get selection state for current view
    pub fn current_selection(&self) -> &SelectionState {
        self.selection
            .get(&self.current_view)
            .expect("selection state should exist for all views")
    }

    /// Get mutable selection state for current view
    pub fn current_selection_mut(&mut self) -> &mut SelectionState {
        self.selection
            .get_mut(&self.current_view)
            .expect("selection state should exist for all views")
    }

    /// Get items for current view
    pub fn current_items(&self) -> &[ResourceItem] {
        match self.current_view {
            ResourceView::Plans => &self.plans,
            ResourceView::Specs => &self.specs,
            ResourceView::Phases => &self.phases,
            ResourceView::Ralphs => &self.ralphs,
            _ => &[], // Analytics views don't have resource items
        }
    }

    /// Get the currently selected item
    pub fn selected_item(&self) -> Option<&ResourceItem> {
        let items = self.current_items();
        let selection = self.current_selection();
        items.get(selection.selected_index)
    }

    /// Push current view to history and navigate to new view
    pub fn navigate_to(&mut self, view: ResourceView) {
        if self.current_view != view {
            // Save current state to history
            let snapshot = ViewSnapshot {
                view: self.current_view,
                selection: self.current_selection().clone(),
            };
            self.view_history.push(snapshot);
            self.current_view = view;
        }
    }

    /// Navigate back to previous view
    pub fn navigate_back(&mut self) -> bool {
        if let Some(snapshot) = self.view_history.pop() {
            self.current_view = snapshot.view;
            self.selection.insert(snapshot.view, snapshot.selection);
            true
        } else {
            false
        }
    }

    /// Drill down from current selection (e.g., Plan -> Specs for that plan)
    pub fn drill_down(&mut self) {
        let selected = self.selected_item().cloned();
        if let Some(item) = selected {
            match self.current_view {
                ResourceView::Plans => {
                    // Navigate to specs filtered by this plan
                    self.navigate_to(ResourceView::Specs);
                    if let Some(selection) = self.selection.get_mut(&ResourceView::Specs) {
                        selection.parent_filter = Some(item.id);
                        selection.selected_index = 0;
                        selection.scroll_offset = 0;
                    }
                }
                ResourceView::Specs => {
                    // Navigate to phases for this spec
                    self.navigate_to(ResourceView::Phases);
                    if let Some(selection) = self.selection.get_mut(&ResourceView::Phases) {
                        selection.parent_filter = Some(item.id);
                        selection.selected_index = 0;
                        selection.scroll_offset = 0;
                    }
                }
                ResourceView::Phases => {
                    // Navigate to ralphs for this phase
                    self.navigate_to(ResourceView::Ralphs);
                    if let Some(selection) = self.selection.get_mut(&ResourceView::Ralphs) {
                        selection.parent_filter = Some(item.id);
                        selection.selected_index = 0;
                        selection.scroll_offset = 0;
                    }
                }
                _ => {}
            }
        }
    }

    /// Set error message (will be displayed in status bar)
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error_message = Some(message.into());
    }

    /// Clear error message
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Tick the frame counter
    pub fn tick(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_view_from_command() {
        assert_eq!(ResourceView::from_command("plans"), Some(ResourceView::Plans));
        assert_eq!(ResourceView::from_command("specs"), Some(ResourceView::Specs));
        assert_eq!(ResourceView::from_command("ralphs"), Some(ResourceView::Ralphs));
        assert_eq!(ResourceView::from_command("loops"), Some(ResourceView::Ralphs));
        assert_eq!(ResourceView::from_command("deps"), Some(ResourceView::Dependencies));
        assert_eq!(ResourceView::from_command("invalid"), None);
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

        // Jump to first
        selection.select_first();
        assert_eq!(selection.selected_index, 0);
    }

    #[test]
    fn test_selection_state_clamp() {
        let mut selection = SelectionState {
            selected_index: 100,
            scroll_offset: 50,
            parent_filter: None,
        };

        selection.clamp(10);
        assert_eq!(selection.selected_index, 9);
        assert!(selection.scroll_offset <= 9);

        selection.clamp(0);
        assert_eq!(selection.selected_index, 0);
        assert_eq!(selection.scroll_offset, 0);
    }

    #[test]
    fn test_app_state_navigation() {
        let mut state = AppState::new();
        assert_eq!(state.current_view, ResourceView::Plans);
        assert!(state.view_history.is_empty());

        // Navigate to specs
        state.navigate_to(ResourceView::Specs);
        assert_eq!(state.current_view, ResourceView::Specs);
        assert_eq!(state.view_history.len(), 1);

        // Navigate back
        assert!(state.navigate_back());
        assert_eq!(state.current_view, ResourceView::Plans);
        assert!(state.view_history.is_empty());

        // Can't go back further
        assert!(!state.navigate_back());
    }

    #[test]
    fn test_layout_mode_cycle() {
        let mode = LayoutMode::Dashboard;
        assert_eq!(mode.next(), LayoutMode::Split);
        assert_eq!(mode.next().next(), LayoutMode::Grid);
        assert_eq!(mode.next().next().next(), LayoutMode::Focus);
        assert_eq!(mode.next().next().next().next(), LayoutMode::Dashboard);
    }

    #[test]
    fn test_resource_item_truncation() {
        let item = ResourceItem {
            id: "test".to_string(),
            name: "This is a very long resource name that needs truncation".to_string(),
            resource_type: "plan".to_string(),
            status: "running".to_string(),
            parent_id: None,
            iteration: None,
            progress: None,
            last_activity: None,
            needs_attention: false,
            attention_reason: None,
        };

        assert_eq!(item.truncated_name(10), "This is...");
        assert_eq!(item.truncated_name(100), item.name);
        assert_eq!(item.truncated_name(3), "Thi");
    }

    #[test]
    fn test_filter_state() {
        let mut filter = FilterState::default();
        assert!(!filter.is_active());

        filter.text = "test".to_string();
        assert!(filter.is_active());

        filter.clear();
        assert!(!filter.is_active());
    }
}
