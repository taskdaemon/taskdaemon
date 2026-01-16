//! TUI Runner - main loop that owns terminal and polls StateManager
//!
//! The TuiRunner is responsible for:
//! - Initializing and restoring the terminal
//! - Polling StateManager for data updates (1s interval)
//! - Dispatching events to App for handling
//! - Rendering at ~30 FPS
//! - Processing REPL input with LLM streaming

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eyre::Result;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::llm::{CompletionRequest, ContentBlock, LlmClient, Message, StopReason, StreamChunk, ToolDefinition};
use crate::state::StateManager;
use crate::tools::{ToolContext, ToolExecutor};

use super::Tui;
use super::app::App;
use super::events::{Event, EventHandler};
use super::state::{
    DescribeData, ExecutionInfo, ExecutionItem, LogEntry, PendingAction, RecordItem, ReplMessage, View,
};
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

    // === REPL state ===
    /// LLM client for REPL interactions
    llm_client: Option<Arc<dyn LlmClient>>,
    /// Tool executor for REPL tool calls
    tool_executor: ToolExecutor,
    /// Working directory for REPL tools
    worktree: PathBuf,
    /// LLM conversation history (separate from display history)
    repl_conversation: Vec<Message>,
    /// System prompt for REPL
    system_prompt: String,
    /// Receiver for stream chunks (populated during streaming)
    stream_rx: Option<mpsc::Receiver<StreamChunk>>,
}

impl TuiRunner {
    /// Create a new TuiRunner without StateManager (for testing/standalone mode)
    pub fn new(terminal: Tui) -> Self {
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let system_prompt = Self::build_system_prompt(&worktree);

        Self {
            app: App::new(),
            terminal,
            state_manager: None,
            event_handler: EventHandler::new(Duration::from_millis(33)), // ~30 FPS
            last_refresh: Instant::now(),
            llm_client: None,
            tool_executor: ToolExecutor::standard(),
            worktree,
            repl_conversation: Vec::new(),
            system_prompt,
            stream_rx: None,
        }
    }

    /// Create a new TuiRunner with StateManager connection
    pub fn with_state_manager(terminal: Tui, state_manager: StateManager) -> Self {
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let system_prompt = Self::build_system_prompt(&worktree);

        Self {
            app: App::new(),
            terminal,
            state_manager: Some(state_manager),
            event_handler: EventHandler::new(Duration::from_millis(33)),
            last_refresh: Instant::now() - DATA_REFRESH_INTERVAL, // Force immediate refresh
            llm_client: None,
            tool_executor: ToolExecutor::standard(),
            worktree,
            repl_conversation: Vec::new(),
            system_prompt,
            stream_rx: None,
        }
    }

    /// Create a new TuiRunner with LLM client for REPL
    pub fn with_llm_client(terminal: Tui, state_manager: Option<StateManager>, llm_client: Arc<dyn LlmClient>) -> Self {
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let system_prompt = Self::build_system_prompt(&worktree);

        Self {
            app: App::new(),
            terminal,
            state_manager,
            event_handler: EventHandler::new(Duration::from_millis(33)),
            last_refresh: Instant::now() - DATA_REFRESH_INTERVAL,
            llm_client: Some(llm_client),
            tool_executor: ToolExecutor::standard(),
            worktree,
            repl_conversation: Vec::new(),
            system_prompt,
            stream_rx: None,
        }
    }

    /// Build the system prompt for REPL
    fn build_system_prompt(worktree: &Path) -> String {
        format!(
            r#"You are an AI coding assistant in an interactive REPL.

You have access to the following tools:
- read_file: Read file contents
- write_file: Create or overwrite a file
- edit_file: Make targeted edits to a file
- list_directory: List files in a directory
- glob: Find files matching a pattern
- grep: Search file contents with regex
- run_command: Execute shell commands

Guidelines:
- Execute tools directly when needed - don't ask for permission
- Be concise in your responses
- Show file contents when asked to read files
- Explain what you're doing before executing tools

Working directory: {}"#,
            worktree.display()
        )
    }

    /// Get tool definitions for the REPL
    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        let tool_names = vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "edit_file".to_string(),
            "list_directory".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "run_command".to_string(),
        ];

        self.tool_executor.definitions_for(&tool_names)
    }

    /// Run the TUI main loop
    pub async fn run(&mut self) -> Result<()> {
        // Fetch initial data if we have a state manager
        if self.state_manager.is_some() {
            self.refresh_data().await?;
        }

        loop {
            // Draw the UI
            self.terminal.draw(|frame| views::render(self.app.state(), frame))?;

            // Handle events
            match self.event_handler.next().await? {
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

        // Check for pending REPL submit
        if let Some(input) = self.app.state_mut().pending_repl_submit.take() {
            self.process_repl_input(&input).await;
        }

        // Process streaming chunks if we're streaming
        self.process_stream_chunks().await;

        // Check for pending task to start
        if let Some(task) = self.app.state_mut().pending_task.take() {
            self.start_task(&task).await;
        }

        // Check for pending action (cancel/pause/resume)
        if let Some(action) = self.app.state_mut().pending_action.take() {
            self.execute_action(action).await;
        }

        // Refresh data if interval has elapsed
        if self.state_manager.is_some() && self.last_refresh.elapsed() >= DATA_REFRESH_INTERVAL {
            self.refresh_data().await?;
            self.last_refresh = Instant::now();
        }

        Ok(())
    }

    /// Process pending stream chunks from LLM
    async fn process_stream_chunks(&mut self) {
        if let Some(rx) = &mut self.stream_rx {
            // Try to receive all available chunks without blocking
            while let Ok(chunk) = rx.try_recv() {
                match chunk {
                    StreamChunk::TextDelta(text) => {
                        self.app.state_mut().repl_response_buffer.push_str(&text);
                    }
                    StreamChunk::ToolUseStart { name, .. } => {
                        // Show tool call in response buffer
                        self.app
                            .state_mut()
                            .repl_response_buffer
                            .push_str(&format!("\n[calling {}]", name));
                    }
                    StreamChunk::Error(err) => {
                        self.app.state_mut().repl_history.push(ReplMessage::error(err));
                    }
                    _ => {}
                }
            }
        }
    }

    /// Process REPL input - call LLM with streaming
    async fn process_repl_input(&mut self, input: &str) {
        // Check if we have an LLM client
        let llm = match &self.llm_client {
            Some(llm) => Arc::clone(llm),
            None => {
                self.app.state_mut().repl_history.push(ReplMessage::error(
                    "No LLM client configured. Start with daemon or set ANTHROPIC_API_KEY.",
                ));
                return;
            }
        };

        // Add user message to display history
        self.app.state_mut().repl_history.push(ReplMessage::user(input));

        // Add user message to LLM conversation
        self.repl_conversation.push(Message::user(input));

        // Set streaming state
        self.app.state_mut().repl_streaming = true;
        self.app.state_mut().repl_response_buffer.clear();

        // Create channel for streaming
        let (tx, rx) = mpsc::channel::<StreamChunk>(100);
        self.stream_rx = Some(rx);

        // Build request
        let request = CompletionRequest {
            system_prompt: self.system_prompt.clone(),
            messages: self.repl_conversation.clone(),
            tools: self.get_tool_definitions(),
            max_tokens: 4096,
        };

        // Call the LLM - this is the main processing loop
        match self.call_llm_loop(llm, request, tx).await {
            Ok((final_text, new_conversation)) => {
                // Update conversation with tool calls/results
                self.repl_conversation = new_conversation;

                // Add assistant response to display history
                if !final_text.is_empty() {
                    self.app
                        .state_mut()
                        .repl_history
                        .push(ReplMessage::assistant(&final_text));
                }
            }
            Err(e) => {
                self.app
                    .state_mut()
                    .repl_history
                    .push(ReplMessage::error(format!("LLM error: {}", e)));
            }
        }

        // Clear streaming state
        self.app.state_mut().repl_streaming = false;
        self.app.state_mut().repl_response_buffer.clear();
        self.stream_rx = None;
    }

    /// Call LLM with tool use loop
    async fn call_llm_loop(
        &mut self,
        llm: Arc<dyn LlmClient>,
        initial_request: CompletionRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(String, Vec<Message>)> {
        let mut request = initial_request;
        let mut conversation = request.messages.clone();
        let mut final_text = String::new();

        loop {
            // Call LLM with streaming
            let response = llm
                .stream(request.clone(), tx.clone())
                .await
                .map_err(|e| eyre::eyre!("LLM error: {}", e))?;

            // Capture any text content
            if let Some(ref text) = response.content {
                final_text = text.clone();
            }

            match response.stop_reason {
                StopReason::EndTurn => {
                    // Done - add assistant response to conversation
                    if let Some(ref content) = response.content {
                        conversation.push(Message::assistant(content));
                    }
                    break;
                }
                StopReason::ToolUse => {
                    // Execute tools and continue
                    if response.tool_calls.is_empty() {
                        break;
                    }

                    // Build assistant message with tool uses
                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    if let Some(ref content) = response.content {
                        blocks.push(ContentBlock::text(content));
                    }
                    for tc in &response.tool_calls {
                        blocks.push(ContentBlock::ToolUse {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            input: tc.input.clone(),
                        });
                    }
                    conversation.push(Message::assistant_blocks(blocks));

                    // Execute tools and collect results
                    let ctx = ToolContext::new_unsandboxed(self.worktree.clone(), "repl".to_string());
                    let mut result_blocks: Vec<ContentBlock> = Vec::new();

                    for tc in &response.tool_calls {
                        // Add tool call to display history
                        self.app
                            .state_mut()
                            .repl_history
                            .push(ReplMessage::tool_result(&tc.name, format!("Running {}...", tc.name)));

                        let result = self.tool_executor.execute(tc, &ctx).await;

                        // Add result to display history
                        let display_content = if result.content.len() > 500 {
                            format!("{}... ({} chars)", &result.content[..500], result.content.len())
                        } else {
                            result.content.clone()
                        };

                        self.app
                            .state_mut()
                            .repl_history
                            .push(ReplMessage::tool_result(&tc.name, display_content));

                        result_blocks.push(ContentBlock::tool_result(&tc.id, &result.content, result.is_error));
                    }

                    // Add tool results to conversation
                    conversation.push(Message::user_blocks(result_blocks));

                    // Update request for next iteration
                    request = CompletionRequest {
                        system_prompt: self.system_prompt.clone(),
                        messages: conversation.clone(),
                        tools: self.get_tool_definitions(),
                        max_tokens: 4096,
                    };
                }
                StopReason::MaxTokens => {
                    if let Some(ref content) = response.content {
                        conversation.push(Message::assistant(content));
                    }
                    self.app
                        .state_mut()
                        .repl_history
                        .push(ReplMessage::error("[Response truncated - max tokens reached]"));
                    break;
                }
                StopReason::StopSequence => {
                    if let Some(ref content) = response.content {
                        conversation.push(Message::assistant(content));
                    }
                    break;
                }
            }
        }

        Ok((final_text, conversation))
    }

    /// Start a new plan loop for the given task
    async fn start_task(&mut self, task: &str) {
        let state_manager = match &self.state_manager {
            Some(sm) => sm,
            None => {
                self.app.state_mut().set_error("No state manager - cannot create loop");
                return;
            }
        };

        debug!("Starting plan loop: {}", task);

        // Create a plan execution - "plan" is always the entry point
        let execution = crate::domain::LoopExecution::new("plan", task);

        match state_manager.create_execution(execution).await {
            Ok(id) => {
                debug!("Created plan loop {}", id);
                // Force a refresh to show the new loop
                self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
            }
            Err(e) => {
                warn!("Failed to create loop: {}", e);
                self.app.state_mut().set_error(format!("Failed to create task: {}", e));
            }
        }
    }

    /// Execute a pending action (cancel/pause/resume)
    async fn execute_action(&mut self, action: PendingAction) {
        let state_manager = match &self.state_manager {
            Some(sm) => sm,
            None => {
                self.app
                    .state_mut()
                    .set_error("No state manager - cannot execute action");
                return;
            }
        };

        match action {
            PendingAction::CancelLoop(id) => {
                debug!("Cancelling loop: {}", id);
                match state_manager.cancel_execution(&id).await {
                    Ok(()) => {
                        debug!("Cancelled loop {}", id);
                        // Force a refresh to show updated status
                        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
                    }
                    Err(e) => {
                        warn!("Failed to cancel loop: {}", e);
                        self.app.state_mut().set_error(format!("Failed to cancel: {}", e));
                    }
                }
            }
            PendingAction::PauseLoop(id) => {
                debug!("Pausing loop: {}", id);
                match state_manager.pause_execution(&id).await {
                    Ok(()) => {
                        debug!("Paused loop {}", id);
                        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
                    }
                    Err(e) => {
                        warn!("Failed to pause loop: {}", e);
                        self.app.state_mut().set_error(format!("Failed to pause: {}", e));
                    }
                }
            }
            PendingAction::ResumeLoop(id) => {
                debug!("Resuming loop: {}", id);
                match state_manager.resume_execution(&id).await {
                    Ok(()) => {
                        debug!("Resumed loop {}", id);
                        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
                    }
                    Err(e) => {
                        warn!("Failed to resume loop: {}", e);
                        self.app.state_mut().set_error(format!("Failed to resume: {}", e));
                    }
                }
            }
            PendingAction::DeleteExecution(id) => {
                debug!("Deleting execution: {}", id);
                match state_manager.delete_execution(&id).await {
                    Ok(()) => {
                        debug!("Deleted execution {}", id);
                        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
                    }
                    Err(e) => {
                        warn!("Failed to delete execution: {}", e);
                        self.app.state_mut().set_error(format!("Failed to delete: {}", e));
                    }
                }
            }
        }
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

        // Sync Loop records
        match state_manager.list_loops(None, None, None).await {
            Ok(loops) => {
                let items: Vec<RecordItem> = loops
                    .iter()
                    .map(|l| {
                        let phases_progress = if !l.phases.is_empty() {
                            let complete = l.phases.iter().filter(|p| p.is_complete()).count();
                            format!("{}/{}", complete, l.phases.len())
                        } else {
                            "-".to_string()
                        };

                        RecordItem {
                            id: l.id.clone(),
                            title: l.title.clone(),
                            loop_type: l.r#type.clone(),
                            status: l.status.to_string(),
                            parent_id: l.parent.clone(),
                            children_count: 0, // TODO: count children
                            phases_progress,
                            created: format_time_ago(l.created_at),
                        }
                    })
                    .collect();

                let state = self.app.state_mut();
                state.total_records = items.len();
                state.records = items;
            }
            Err(e) => {
                warn!("Failed to fetch loops: {}", e);
            }
        }

        // Sync loop executions
        match state_manager.list_executions(None, None).await {
            Ok(executions) => {
                let items: Vec<ExecutionItem> = executions
                    .iter()
                    .map(|e| {
                        // Only show duration for running items
                        let duration = if e.status == crate::domain::LoopExecutionStatus::Running {
                            format_duration(e.created_at)
                        } else {
                            "-".to_string()
                        };
                        let progress = e.progress.lines().last().unwrap_or("").to_string();

                        ExecutionItem {
                            id: e.id.clone(),
                            name: format!("{} ({})", e.loop_type, &e.id[..8.min(e.id.len())]),
                            loop_type: e.loop_type.clone(),
                            iteration: format!("{}/10", e.iteration), // TODO: get max from config
                            status: e.status.to_string(),
                            duration,
                            parent_id: e.parent.clone(),
                            progress,
                        }
                    })
                    .collect();

                let state = self.app.state_mut();
                state.executions_active = items.iter().filter(|r| r.status == "running").count();
                state.executions_complete = items.iter().filter(|r| r.status == "complete").count();
                state.executions_failed = items.iter().filter(|r| r.status == "failed").count();
                state.executions = items;
            }
            Err(e) => {
                warn!("Failed to fetch executions: {}", e);
            }
        }

        // Update last refresh timestamp
        self.app.state_mut().last_refresh = taskstore::now_ms();

        // Clamp selections to valid ranges
        let state = self.app.state_mut();
        let records_len = state.records.len();
        let executions_len = state.executions.len();

        state.records_selection.clamp(records_len);
        state.executions_selection.clamp(executions_len);

        // Load view-specific data
        self.load_view_data().await?;

        Ok(())
    }

    /// Load data specific to the current view
    async fn load_view_data(&mut self) -> Result<()> {
        let state_manager = match &self.state_manager {
            Some(sm) => sm,
            None => return Ok(()),
        };

        let view = self.app.state().current_view.clone();

        match view {
            View::Logs { ref target_id } => {
                // Load logs for the target (try execution first, then loop record)
                if let Ok(Some(exec)) = state_manager.get_execution(target_id).await {
                    let entries: Vec<LogEntry> = exec
                        .progress
                        .lines()
                        .enumerate()
                        .map(|(i, line)| {
                            let is_error = line.contains("ERROR") || line.contains("error:");
                            let is_stdout = line.contains("STDOUT:") || line.starts_with('>');
                            LogEntry {
                                iteration: (i / 10 + 1) as u32, // Rough estimate
                                text: line.to_string(),
                                is_error,
                                is_stdout,
                            }
                        })
                        .collect();

                    self.app.state_mut().logs = entries;
                }
            }
            View::Describe {
                ref target_id,
                ref target_type,
            } => {
                // Load describe data - try execution first, then loop record
                let data = if let Ok(Some(exec)) = state_manager.get_execution(target_id).await {
                    // It's an execution
                    let duration = if exec.status == crate::domain::LoopExecutionStatus::Running {
                        format_duration(exec.created_at)
                    } else {
                        "-".to_string()
                    };

                    Some(DescribeData {
                        id: exec.id.clone(),
                        loop_type: exec.loop_type.clone(),
                        title: format!("{} execution", exec.loop_type),
                        status: exec.status.to_string(),
                        parent_id: exec.parent.clone(),
                        created: format_timestamp(exec.created_at),
                        updated: format_timestamp(exec.updated_at),
                        fields: if let Some(ref err) = exec.last_error {
                            vec![("Last Error".to_string(), err.clone())]
                        } else {
                            vec![]
                        },
                        children: vec![],
                        execution: Some(ExecutionInfo {
                            id: exec.id.clone(),
                            iteration: format!("{}/10", exec.iteration),
                            duration,
                            progress: exec.progress.lines().last().unwrap_or("").to_string(),
                        }),
                    })
                } else if let Ok(Some(record)) = state_manager.get_loop(target_id).await {
                    // It's a Loop record
                    Some(DescribeData {
                        id: record.id.clone(),
                        loop_type: record.r#type.clone(),
                        title: record.title.clone(),
                        status: record.status.to_string(),
                        parent_id: record.parent.clone(),
                        created: format_timestamp(record.created_at),
                        updated: format_timestamp(record.updated_at),
                        fields: vec![(
                            "File".to_string(),
                            record.file.clone().unwrap_or_else(|| "-".to_string()),
                        )],
                        children: vec![], // TODO: load children
                        execution: None,
                    })
                } else {
                    None
                };

                let _ = target_type; // Used for context in future

                self.app.state_mut().describe_data = data;
            }
            _ => {}
        }

        Ok(())
    }
}

/// Format a timestamp as a human-readable "time ago" string
fn format_time_ago(timestamp_ms: i64) -> String {
    let now = taskstore::now_ms();
    let diff_ms = now - timestamp_ms;

    if diff_ms < 0 {
        return "just now".to_string();
    }

    let diff_secs = diff_ms / 1000;
    let diff_mins = diff_secs / 60;
    let diff_hours = diff_mins / 60;
    let diff_days = diff_hours / 24;

    if diff_days > 0 {
        format!("{}d ago", diff_days)
    } else if diff_hours > 0 {
        format!("{}h ago", diff_hours)
    } else if diff_mins > 0 {
        format!("{}m ago", diff_mins)
    } else {
        "just now".to_string()
    }
}

/// Format a duration from creation timestamp to now
fn format_duration(created_at_ms: i64) -> String {
    let now = taskstore::now_ms();
    let diff_ms = now - created_at_ms;

    if diff_ms < 0 {
        return "0:00".to_string();
    }

    let total_secs = diff_ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;

    format!("{}:{:02}", mins, secs)
}

/// Format a timestamp as ISO date string
fn format_timestamp(timestamp_ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    let dt = Utc.timestamp_millis_opt(timestamp_ms);
    match dt {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_refresh_interval() {
        assert_eq!(DATA_REFRESH_INTERVAL, Duration::from_secs(1));
    }

    #[test]
    fn test_format_time_ago() {
        let now = taskstore::now_ms();

        // Just now
        assert_eq!(format_time_ago(now), "just now");

        // Minutes ago
        assert_eq!(format_time_ago(now - 5 * 60 * 1000), "5m ago");

        // Hours ago
        assert_eq!(format_time_ago(now - 2 * 60 * 60 * 1000), "2h ago");

        // Days ago
        assert_eq!(format_time_ago(now - 3 * 24 * 60 * 60 * 1000), "3d ago");
    }

    #[test]
    fn test_format_duration() {
        let now = taskstore::now_ms();

        assert_eq!(format_duration(now), "0:00");
        assert_eq!(format_duration(now - 65_000), "1:05");
        assert_eq!(format_duration(now - 3_600_000), "60:00");
    }
}
