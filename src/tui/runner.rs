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
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::llm::{
    CompletionRequest, ContentBlock, LlmClient, Message, StopReason, StreamChunk, ToolCall, ToolDefinition,
};
use crate::state::StateManager;
use crate::tools::{ToolContext, ToolExecutor};

use super::Tui;
use super::app::App;
use super::events::{Event, EventHandler};
use super::state::{
    DescribeData, ExecutionInfo, ExecutionItem, LogEntry, PendingAction, PlanCreateRequest, RecordItem, ReplMessage,
    ReplMode, ReplRole, View,
};
use super::views;

/// How often to refresh data from StateManager
const DATA_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Result from the background LLM task
#[derive(Debug)]
enum LlmTaskResult {
    /// LLM returned with response
    Response {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        stop_reason: StopReason,
    },
    /// Error occurred
    Error(String),
}

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
    /// System prompt for Chat mode REPL
    chat_system_prompt: String,
    /// System prompt for Plan mode REPL
    plan_system_prompt: String,
    /// Receiver for stream chunks (populated during streaming)
    stream_rx: Option<mpsc::Receiver<StreamChunk>>,
    /// Receiver for LLM task results
    llm_result_rx: Option<mpsc::Receiver<LlmTaskResult>>,
    /// Handle to the background LLM task
    llm_task: Option<JoinHandle<()>>,
}

impl TuiRunner {
    /// Create a new TuiRunner without StateManager (for testing/standalone mode)
    pub fn new(terminal: Tui) -> Self {
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let chat_system_prompt = Self::build_chat_system_prompt(&worktree);
        let plan_system_prompt = Self::build_plan_system_prompt(&worktree);

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
            chat_system_prompt,
            plan_system_prompt,
            stream_rx: None,
            llm_result_rx: None,
            llm_task: None,
        }
    }

    /// Create a new TuiRunner with StateManager connection
    pub fn with_state_manager(terminal: Tui, state_manager: StateManager) -> Self {
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let chat_system_prompt = Self::build_chat_system_prompt(&worktree);
        let plan_system_prompt = Self::build_plan_system_prompt(&worktree);

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
            chat_system_prompt,
            plan_system_prompt,
            stream_rx: None,
            llm_result_rx: None,
            llm_task: None,
        }
    }

    /// Create a new TuiRunner with LLM client for REPL
    pub fn with_llm_client(terminal: Tui, state_manager: Option<StateManager>, llm_client: Arc<dyn LlmClient>) -> Self {
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let chat_system_prompt = Self::build_chat_system_prompt(&worktree);
        let plan_system_prompt = Self::build_plan_system_prompt(&worktree);

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
            chat_system_prompt,
            plan_system_prompt,
            stream_rx: None,
            llm_result_rx: None,
            llm_task: None,
        }
    }

    /// Build the system prompt for Chat mode REPL
    fn build_chat_system_prompt(worktree: &Path) -> String {
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

    /// Build the system prompt for Plan mode REPL
    fn build_plan_system_prompt(worktree: &Path) -> String {
        format!(
            r#"You are a senior software architect helping gather requirements for a technical plan.

Your role is to:
1. Ask clarifying questions about the user's goals
2. Identify missing details (scope, constraints, dependencies)
3. Suggest considerations they may have missed
4. Summarize the requirements when asked

Guidelines:
- Keep responses concise and focused
- Ask one or two questions at a time
- Acknowledge good answers before moving on
- When requirements seem complete, suggest using /create to generate the plan

Do NOT generate the full plan during this conversation.
Focus on gathering comprehensive requirements first.

You have access to these tools for exploring the codebase:
- read_file: Read file contents
- list_directory: List files in a directory
- glob: Find files matching a pattern
- grep: Search file contents with regex

Working directory: {}"#,
            worktree.display()
        )
    }

    /// Get the current system prompt based on REPL mode
    fn current_system_prompt(&self) -> &str {
        match self.app.state().repl_mode {
            ReplMode::Chat => &self.chat_system_prompt,
            ReplMode::Plan => &self.plan_system_prompt,
        }
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

        // Check for pending REPL submit - spawn background task
        if let Some(input) = self.app.state_mut().pending_repl_submit.take() {
            self.start_repl_request(&input);
        }

        // Process streaming chunks if we're streaming
        self.process_stream_chunks();

        // Check for LLM task results
        self.process_llm_results().await;

        // Check for pending task to start
        if let Some(task) = self.app.state_mut().pending_task.take() {
            self.start_task(&task).await;
        }

        // Check for pending plan creation
        if let Some(request) = self.app.state_mut().pending_plan_create.take() {
            self.create_plan_draft(request).await;
        }

        // Check for pending action (cancel/pause/resume/start draft)
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

    /// Process pending stream chunks from LLM (non-blocking)
    fn process_stream_chunks(&mut self) {
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

    /// Process LLM task results (non-blocking check)
    async fn process_llm_results(&mut self) {
        // Collect results first to avoid borrow conflicts
        let results: Vec<LlmTaskResult> = if let Some(rx) = &mut self.llm_result_rx {
            let mut collected = Vec::new();
            while let Ok(result) = rx.try_recv() {
                collected.push(result);
            }
            collected
        } else {
            return;
        };

        // Now process each result
        for result in results {
            match result {
                LlmTaskResult::Response {
                    content,
                    tool_calls,
                    stop_reason,
                } => {
                    // Handle the response based on stop reason
                    match stop_reason {
                        StopReason::EndTurn => {
                            // Done - add response to history and conversation
                            if let Some(ref text) = content {
                                // Add streamed content to history
                                let response_text = if self.app.state().repl_response_buffer.is_empty() {
                                    text.clone()
                                } else {
                                    std::mem::take(&mut self.app.state_mut().repl_response_buffer)
                                };
                                self.app
                                    .state_mut()
                                    .repl_history
                                    .push(ReplMessage::assistant(&response_text));
                                self.repl_conversation.push(Message::assistant(&response_text));
                            }
                            self.finish_streaming();
                        }
                        StopReason::ToolUse => {
                            // Execute tools and continue
                            self.handle_tool_calls(content, tool_calls).await;
                        }
                        StopReason::MaxTokens => {
                            if let Some(ref text) = content {
                                let response_text = std::mem::take(&mut self.app.state_mut().repl_response_buffer);
                                if !response_text.is_empty() {
                                    self.app
                                        .state_mut()
                                        .repl_history
                                        .push(ReplMessage::assistant(&response_text));
                                }
                                self.repl_conversation.push(Message::assistant(text));
                            }
                            self.app
                                .state_mut()
                                .repl_history
                                .push(ReplMessage::error("[Response truncated - max tokens]"));
                            self.finish_streaming();
                        }
                        StopReason::StopSequence => {
                            if let Some(ref text) = content {
                                let response_text = std::mem::take(&mut self.app.state_mut().repl_response_buffer);
                                if !response_text.is_empty() {
                                    self.app
                                        .state_mut()
                                        .repl_history
                                        .push(ReplMessage::assistant(&response_text));
                                }
                                self.repl_conversation.push(Message::assistant(text));
                            }
                            self.finish_streaming();
                        }
                    }
                }
                LlmTaskResult::Error(err) => {
                    self.app
                        .state_mut()
                        .repl_history
                        .push(ReplMessage::error(format!("LLM error: {}", err)));
                    self.finish_streaming();
                }
            }
        }
    }

    /// Finish streaming state
    fn finish_streaming(&mut self) {
        self.app.state_mut().repl_streaming = false;
        self.app.state_mut().repl_response_buffer.clear();
        self.stream_rx = None;
        self.llm_result_rx = None;
        self.llm_task = None;
    }

    /// Handle tool calls from LLM response
    async fn handle_tool_calls(&mut self, content: Option<String>, tool_calls: Vec<ToolCall>) {
        if tool_calls.is_empty() {
            self.finish_streaming();
            return;
        }

        // Build assistant message with tool uses
        let mut blocks: Vec<ContentBlock> = Vec::new();
        if let Some(ref text) = content {
            blocks.push(ContentBlock::text(text));
            // Show text content if any
            if !text.is_empty() {
                let response_text = std::mem::take(&mut self.app.state_mut().repl_response_buffer);
                if !response_text.is_empty() {
                    self.app
                        .state_mut()
                        .repl_history
                        .push(ReplMessage::assistant(&response_text));
                }
            }
        }
        for tc in &tool_calls {
            blocks.push(ContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
        }
        self.repl_conversation.push(Message::assistant_blocks(blocks));

        // Execute tools and collect results
        let ctx = ToolContext::new_unsandboxed(self.worktree.clone(), "repl".to_string());
        let mut result_blocks: Vec<ContentBlock> = Vec::new();

        for tc in &tool_calls {
            // Show tool call
            self.app
                .state_mut()
                .repl_history
                .push(ReplMessage::tool_result(&tc.name, format!("Running {}...", tc.name)));

            let result = self.tool_executor.execute(tc, &ctx).await;

            // Show result (truncated)
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
        self.repl_conversation.push(Message::user_blocks(result_blocks));

        // Clear response buffer for next LLM turn
        self.app.state_mut().repl_response_buffer.clear();

        // Continue with another LLM call
        self.continue_llm_request();
    }

    /// Start a new REPL request (spawns background task)
    fn start_repl_request(&mut self, input: &str) {
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

        // Create channel for streaming chunks
        let (stream_tx, stream_rx) = mpsc::channel::<StreamChunk>(100);
        self.stream_rx = Some(stream_rx);

        // Create channel for LLM task result
        let (result_tx, result_rx) = mpsc::channel::<LlmTaskResult>(1);
        self.llm_result_rx = Some(result_rx);

        // Build request (use current system prompt based on mode)
        let request = CompletionRequest {
            system_prompt: self.current_system_prompt().to_string(),
            messages: self.repl_conversation.clone(),
            tools: self.get_tool_definitions(),
            max_tokens: 4096,
        };

        // Spawn background task
        self.llm_task = Some(tokio::spawn(async move {
            let result = match llm.stream(request, stream_tx).await {
                Ok(response) => LlmTaskResult::Response {
                    content: response.content,
                    tool_calls: response.tool_calls,
                    stop_reason: response.stop_reason,
                },
                Err(e) => LlmTaskResult::Error(e.to_string()),
            };
            let _ = result_tx.send(result).await;
        }));
    }

    /// Continue LLM request after tool execution (spawns background task)
    fn continue_llm_request(&mut self) {
        // Check if we have an LLM client
        let llm = match &self.llm_client {
            Some(llm) => Arc::clone(llm),
            None => {
                self.finish_streaming();
                return;
            }
        };

        // Create channel for streaming chunks
        let (stream_tx, stream_rx) = mpsc::channel::<StreamChunk>(100);
        self.stream_rx = Some(stream_rx);

        // Create channel for LLM task result
        let (result_tx, result_rx) = mpsc::channel::<LlmTaskResult>(1);
        self.llm_result_rx = Some(result_rx);

        // Build request with current conversation (includes tool results)
        let request = CompletionRequest {
            system_prompt: self.current_system_prompt().to_string(),
            messages: self.repl_conversation.clone(),
            tools: self.get_tool_definitions(),
            max_tokens: 4096,
        };

        // Spawn background task
        self.llm_task = Some(tokio::spawn(async move {
            let result = match llm.stream(request, stream_tx).await {
                Ok(response) => LlmTaskResult::Response {
                    content: response.content,
                    tool_calls: response.tool_calls,
                    stop_reason: response.stop_reason,
                },
                Err(e) => LlmTaskResult::Error(e.to_string()),
            };
            let _ = result_tx.send(result).await;
        }));
    }

    /// Generate a short title from task description using LLM
    async fn generate_title(&self, task: &str) -> Option<String> {
        let llm = self.llm_client.as_ref()?;

        let request = CompletionRequest {
            system_prompt: "Extract 2-4 key words from the task description to create a short title. \
                           Output ONLY the title words separated by spaces, nothing else. \
                           Examples: 'Add OAuth' 'Fix Login Bug' 'Update API Docs' 'Refactor Tests'"
                .to_string(),
            messages: vec![Message::user(task)],
            tools: vec![],
            max_tokens: 50,
        };

        match llm.complete(request).await {
            Ok(response) => response.content.map(|s| s.trim().to_string()),
            Err(e) => {
                debug!("Failed to generate title: {}", e);
                None
            }
        }
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

        // Generate a short title via LLM
        let title = self.generate_title(task).await;

        // Create a plan execution - "plan" is always the entry point
        let mut execution = crate::domain::LoopExecution::new("plan", task);
        if let Some(t) = title {
            execution.set_title(t);
        }

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

    /// Create a plan draft from conversation
    async fn create_plan_draft(&mut self, request: PlanCreateRequest) {
        let state_manager = match &self.state_manager {
            Some(sm) => sm,
            None => {
                self.app.state_mut().set_error("No state manager - cannot create plan");
                return;
            }
        };

        let llm = match &self.llm_client {
            Some(llm) => Arc::clone(llm),
            None => {
                self.app
                    .state_mut()
                    .set_error("No LLM client - cannot summarize conversation");
                return;
            }
        };

        debug!("Creating plan draft from {} messages", request.messages.len());

        // Format conversation for summarization
        let conversation_text = request
            .messages
            .iter()
            .map(|msg| {
                let role = match &msg.role {
                    ReplRole::User => "User",
                    ReplRole::Assistant => "Assistant",
                    ReplRole::ToolResult { tool_name } => tool_name.as_str(),
                    ReplRole::Error => "Error",
                };
                format!("{}: {}", role, msg.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Summarize conversation using LLM
        let summarize_prompt = r#"Analyze this conversation and extract a structured requirements summary.

Output format:
## Goal
[One sentence describing what the user wants to build/change]

## Requirements
- [Requirement 1]
- [Requirement 2]
...

## Constraints
- [Constraint 1]
- [Constraint 2]
...

## Key Decisions
- [Decision made during conversation]
...

Be comprehensive but concise. Include all requirements discussed."#;

        let summarize_request = CompletionRequest {
            system_prompt: summarize_prompt.to_string(),
            messages: vec![Message::user(&conversation_text)],
            tools: vec![],
            max_tokens: 2048,
        };

        let summary = match llm.complete(summarize_request).await {
            Ok(response) => response.content.unwrap_or_else(|| "No summary generated".to_string()),
            Err(e) => {
                warn!("Failed to summarize conversation: {}", e);
                self.app
                    .state_mut()
                    .set_error(format!("Failed to summarize conversation: {}. Try again.", e));
                return;
            }
        };

        // Generate a short title
        let title = self
            .generate_title(&summary)
            .await
            .unwrap_or_else(|| "New Plan".to_string());

        // Create the plan execution with Draft status
        let mut execution = crate::domain::LoopExecution::new("plan", &title);
        execution.set_title(title.clone());
        execution.set_status(crate::domain::LoopExecutionStatus::Draft);

        // Store the summary in context
        execution.set_context(serde_json::json!({
            "user-request": summary,
            "conversation-summary": conversation_text,
            "review-pass": 0
        }));

        // Create the execution record
        let exec_id = match state_manager.create_execution(execution).await {
            Ok(id) => id,
            Err(e) => {
                warn!("Failed to create draft execution: {}", e);
                self.app.state_mut().set_error(format!("Failed to create draft: {}", e));
                return;
            }
        };

        // Create the plan directory and file
        let plan_dir = self.worktree.join(".taskdaemon/plans").join(&exec_id);
        if let Err(e) = std::fs::create_dir_all(&plan_dir) {
            warn!("Failed to create plan directory: {}", e);
            self.app
                .state_mut()
                .set_error(format!("Failed to create plan directory: {}", e));
            return;
        }

        // Write the initial plan.md file
        let plan_content = format!(
            r#"# Plan: {}

**Status:** Draft
**Created:** {}
**ID:** {}

{}

---

*This is a draft plan. Review and edit as needed, then use `s` in the Executions view to start execution.*
"#,
            title,
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            exec_id,
            summary
        );

        let plan_path = plan_dir.join("plan.md");
        if let Err(e) = std::fs::write(&plan_path, plan_content) {
            warn!("Failed to write plan file: {}", e);
            self.app
                .state_mut()
                .set_error(format!("Failed to write plan file: {}", e));
            return;
        }

        // Extract hash for display (first 6 chars)
        let hash = &exec_id[..6.min(exec_id.len())];

        // Show success message
        self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
            "Created draft plan: {} ({})\nView in Executions with `2` key or arrow right, or continue chatting.",
            title, hash
        )));

        debug!("Created draft plan: {} at {:?}", exec_id, plan_path);

        // Force refresh to show the new draft
        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
    }

    /// Execute a pending action (cancel/pause/resume/start draft)
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
            PendingAction::StartDraft(id) => {
                debug!("Starting draft: {}", id);
                match state_manager.start_draft(&id).await {
                    Ok(()) => {
                        debug!("Started draft {}", id);
                        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
                    }
                    Err(e) => {
                        warn!("Failed to start draft: {}", e);
                        self.app.state_mut().set_error(format!("Failed to start draft: {}", e));
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

                        // Build name: "019bc8-fix-login-bug" (lowercase, hyphenated)
                        let hash = &e.id[..6.min(e.id.len())];
                        let title_part = if let Some(ref title) = e.title {
                            // Convert title to lowercase slug format
                            title
                                .to_lowercase()
                                .chars()
                                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                                .collect::<String>()
                                .split('-')
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join("-")
                        } else {
                            // Extract slug from ID: format is {hash}-{type}-{slug}
                            e.id.splitn(3, '-').nth(2).unwrap_or(&e.id).to_string()
                        };
                        let name = format!("{}-{}", hash, title_part);

                        ExecutionItem {
                            id: e.id.clone(),
                            name,
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
