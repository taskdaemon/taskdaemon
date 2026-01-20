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
use tracing::{debug, info, trace, warn};

use crate::llm::{
    CompletionRequest, ContentBlock, LlmClient, Message, StopReason, StreamChunk, ToolCall, ToolDefinition,
};
use crate::state::{StateEvent, StateManager, read_state_version};
use crate::tools::{ToolContext, ToolExecutor};

use super::Tui;
use super::app::App;
use super::conversation_log::ConversationLogger;
use super::events::{Event, EventHandler};
use super::state::{
    DaemonStatus, DescribeData, ExecutionInfo, ExecutionItem, LogEntry, PendingAction, PlanCreateRequest, RecordItem,
    ReplMessage, ReplMode, ReplRole, View,
};
use super::views;
use crate::daemon::DaemonManager;

/// How often to refresh data from StateManager (250ms for responsive updates)
const DATA_REFRESH_INTERVAL: Duration = Duration::from_millis(250);

/// Result from the background LLM task
#[derive(Debug)]
enum LlmTaskResult {
    /// LLM returned with response
    Response {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        stop_reason: StopReason,
        input_tokens: u64,
        output_tokens: u64,
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
    /// Max tokens for LLM requests (from config)
    max_tokens: u32,
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
    /// Conversation logger for debug mode
    conversation_logger: ConversationLogger,

    // === Plan creation state ===
    /// Receiver for plan creation progress messages
    plan_progress_rx: Option<mpsc::Receiver<PlanProgress>>,
    /// Handle to the background plan creation task
    plan_task: Option<JoinHandle<()>>,

    // === State change notifications ===
    /// Receiver for instant state change notifications (new executions, etc.)
    state_event_rx: Option<tokio::sync::broadcast::Receiver<StateEvent>>,
    /// Last known state version (for cross-process change detection)
    last_state_version: u64,
}

/// Progress updates from plan creation background task
#[derive(Debug)]
enum PlanProgress {
    /// Plan creation started
    Started,
    /// Streaming text chunk from LLM
    TextChunk(String),
    /// Plan creation completed successfully
    Completed {
        exec_id: String,
        title: String,
        plan_content: String,
    },
    /// Plan creation failed
    Failed { error: String },
}

impl TuiRunner {
    /// Create a new TuiRunner without StateManager (for testing/standalone mode)
    pub fn new(terminal: Tui) -> Self {
        debug!("TuiRunner::new: called (no StateManager)");
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        debug!(?worktree, "TuiRunner::new: worktree");
        let chat_system_prompt = Self::build_chat_system_prompt(&worktree);
        let plan_system_prompt = Self::build_plan_system_prompt(&worktree);

        Self {
            app: App::new(),
            terminal,
            state_manager: None,
            event_handler: EventHandler::new(Duration::from_millis(33)), // ~30 FPS
            last_refresh: Instant::now(),
            llm_client: None,
            max_tokens: 16384, // Default fallback
            tool_executor: ToolExecutor::standard(),
            worktree,
            repl_conversation: Vec::new(),
            chat_system_prompt,
            plan_system_prompt,
            stream_rx: None,
            llm_result_rx: None,
            llm_task: None,
            conversation_logger: ConversationLogger::disabled(),
            plan_progress_rx: None,
            plan_task: None,
            state_event_rx: None,
            last_state_version: 0,
        }
    }

    /// Create a new TuiRunner with StateManager connection
    pub fn with_state_manager(terminal: Tui, state_manager: StateManager) -> Self {
        debug!("TuiRunner::with_state_manager: called");
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        debug!(?worktree, "TuiRunner::with_state_manager: worktree");
        let chat_system_prompt = Self::build_chat_system_prompt(&worktree);
        let plan_system_prompt = Self::build_plan_system_prompt(&worktree);

        // Subscribe to state change events for instant updates
        let state_event_rx = Some(state_manager.subscribe_events());

        Self {
            app: App::new(),
            terminal,
            state_manager: Some(state_manager),
            event_handler: EventHandler::new(Duration::from_millis(33)),
            last_refresh: Instant::now() - DATA_REFRESH_INTERVAL, // Force immediate refresh
            llm_client: None,
            max_tokens: 16384, // Default fallback
            tool_executor: ToolExecutor::standard(),
            worktree,
            repl_conversation: Vec::new(),
            chat_system_prompt,
            plan_system_prompt,
            stream_rx: None,
            llm_result_rx: None,
            llm_task: None,
            conversation_logger: ConversationLogger::disabled(),
            plan_progress_rx: None,
            plan_task: None,
            state_event_rx,
            last_state_version: read_state_version(),
        }
    }

    /// Create a new TuiRunner with LLM client for REPL
    pub fn with_llm_client(
        terminal: Tui,
        state_manager: Option<StateManager>,
        llm_client: Arc<dyn LlmClient>,
        max_tokens: u32,
        log_conversations: bool,
    ) -> Self {
        debug!(
            has_state_manager = state_manager.is_some(),
            max_tokens, log_conversations, "TuiRunner::with_llm_client: called"
        );
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        debug!(?worktree, "TuiRunner::with_llm_client: worktree");
        let chat_system_prompt = Self::build_chat_system_prompt(&worktree);
        let plan_system_prompt = Self::build_plan_system_prompt(&worktree);

        let conversation_logger = if log_conversations {
            debug!("TuiRunner::with_llm_client: enabling conversation logger");
            ConversationLogger::enabled()
        } else {
            debug!("TuiRunner::with_llm_client: disabling conversation logger");
            ConversationLogger::disabled()
        };

        // Subscribe to state change events for instant updates
        let state_event_rx = state_manager.as_ref().map(|sm| sm.subscribe_events());

        Self {
            app: App::new(),
            terminal,
            state_manager,
            event_handler: EventHandler::new(Duration::from_millis(33)),
            last_refresh: Instant::now() - DATA_REFRESH_INTERVAL,
            llm_client: Some(llm_client),
            max_tokens,
            tool_executor: ToolExecutor::standard(),
            worktree,
            repl_conversation: Vec::new(),
            chat_system_prompt,
            plan_system_prompt,
            stream_rx: None,
            llm_result_rx: None,
            llm_task: None,
            conversation_logger,
            plan_progress_rx: None,
            plan_task: None,
            state_event_rx,
            last_state_version: read_state_version(),
        }
    }

    /// Build the system prompt for Chat mode REPL
    fn build_chat_system_prompt(worktree: &Path) -> String {
        debug!(?worktree, "TuiRunner::build_chat_system_prompt: called");
        format!(
            r#"You are an AI coding assistant in an interactive REPL.

You have access to the following tools:
- read: Read file contents
- write: Create or overwrite a file
- edit: Make targeted edits to a file
- list: List files in a directory
- glob: Find files matching a pattern
- grep: Search file contents with regex
- bash: Execute shell commands

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
        debug!(?worktree, "TuiRunner::build_plan_system_prompt: called");
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
- read: Read file contents
- list: List files in a directory
- glob: Find files matching a pattern
- grep: Search file contents with regex

Working directory: {}"#,
            worktree.display()
        )
    }

    /// Get the current system prompt based on REPL mode
    fn current_system_prompt(&self) -> &str {
        debug!(repl_mode = ?self.app.state().repl_mode, "TuiRunner::current_system_prompt: called");
        match self.app.state().repl_mode {
            ReplMode::Chat => {
                debug!("TuiRunner::current_system_prompt: returning chat prompt");
                &self.chat_system_prompt
            }
            ReplMode::Plan => {
                debug!("TuiRunner::current_system_prompt: returning plan prompt");
                &self.plan_system_prompt
            }
        }
    }

    /// Get tool definitions for the REPL
    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        debug!("TuiRunner::get_tool_definitions: called");
        let tool_names = vec![
            "read".to_string(),
            "write".to_string(),
            "edit".to_string(),
            "list".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "bash".to_string(),
        ];

        let definitions = self.tool_executor.definitions_for(&tool_names);
        debug!(count = definitions.len(), "TuiRunner::get_tool_definitions: returning");
        definitions
    }

    /// Run the TUI main loop
    pub async fn run(&mut self) -> Result<()> {
        debug!("TuiRunner::run: called");
        // Fetch initial data if we have a state manager
        if self.state_manager.is_some() {
            debug!("TuiRunner::run: state manager present, refreshing data");
            self.refresh_data().await?;
        }

        debug!("TuiRunner::run: entering main loop");
        loop {
            // Process stream chunks for immediate display
            self.process_stream_chunks();

            // Draw the UI
            self.terminal.draw(|frame| views::render(self.app.state_mut(), frame))?;

            // Wait for either an event OR a plan progress message
            // Stream chunks are processed at start of each loop iteration
            tokio::select! {
                event = self.event_handler.next() => {
                    match event? {
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
                }
                // Handle plan progress messages immediately when they arrive
                Some(progress) = async {
                    if let Some(rx) = &mut self.plan_progress_rx {
                        rx.recv().await
                    } else {
                        std::future::pending::<Option<PlanProgress>>().await
                    }
                } => {
                    self.handle_plan_progress(progress);
                }
            }

            // Check if we should quit
            if self.app.state().should_quit {
                debug!("TuiRunner::run: should_quit is true, breaking");
                break;
            }
        }

        debug!("TuiRunner::run: exiting");
        Ok(())
    }

    /// Handle a single plan progress message
    fn handle_plan_progress(&mut self, progress: PlanProgress) {
        debug!(?progress, "TuiRunner::handle_plan_progress: called");
        match progress {
            PlanProgress::Started => {
                debug!("TuiRunner::handle_plan_progress: Started");
                // Add initial streaming message that will be appended to
                self.app
                    .state_mut()
                    .repl_history
                    .push(ReplMessage::assistant(String::new()));
            }
            PlanProgress::TextChunk(chunk) => {
                debug!(chunk_len = chunk.len(), "TuiRunner::handle_plan_progress: TextChunk");
                // Append chunk to the last message (streaming output)
                if let Some(last) = self.app.state_mut().repl_history.last_mut() {
                    last.content.push_str(&chunk);
                }
            }
            PlanProgress::Completed {
                exec_id,
                title,
                plan_content: _,
            } => {
                debug!(%exec_id, %title, "TuiRunner::handle_plan_progress: Completed");
                info!("Plan creation completed: {} / {}", exec_id, title);
                self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                    "\n\n---\nPlan created: {} ({})\nView in Executions tab (Tab to switch).",
                    title, exec_id
                )));
                // Clear plan creating flag
                self.app.state_mut().plan_creating = false;
                self.plan_progress_rx = None;
                self.plan_task = None;
            }
            PlanProgress::Failed { error } => {
                debug!(%error, "TuiRunner::handle_plan_progress: Failed");
                warn!("Plan creation failed: {}", error);
                self.app
                    .state_mut()
                    .set_error(format!("Plan creation failed: {}", error));
                self.app.state_mut().plan_creating = false;
                self.plan_progress_rx = None;
                self.plan_task = None;
            }
        }
    }

    /// Handle tick event - periodic updates
    async fn handle_tick(&mut self) -> Result<()> {
        debug!("TuiRunner::handle_tick: called");
        self.app.state_mut().tick();

        // Check for pending REPL submit - spawn background task
        if let Some(input) = self.app.state_mut().pending_repl_submit.take() {
            debug!(input_len = input.len(), "TuiRunner::handle_tick: pending REPL submit");
            info!("REPL submit received: {} chars", input.len());
            self.start_repl_request(&input);
        }

        // Process streaming chunks if we're streaming
        self.process_stream_chunks();

        // Check for LLM task results
        self.process_llm_results().await;

        // Check for pending task to start
        if let Some(task) = self.app.state_mut().pending_task.take() {
            debug!(%task, "TuiRunner::handle_tick: pending task");
            info!("Starting task: {}", task);
            self.start_task(&task).await;
        }

        // Check for pending plan creation - spawn background task
        if let Some(request) = self.app.state_mut().pending_plan_create.take() {
            debug!(
                message_count = request.messages.len(),
                "TuiRunner::handle_tick: pending plan create"
            );
            info!("Creating plan from {} messages", request.messages.len());
            self.start_plan_creation(request);
        }

        // Process plan creation progress
        self.process_plan_progress().await;

        // Check for pending action (cancel/pause/resume/start draft)
        if let Some(action) = self.app.state_mut().pending_action.take() {
            debug!(?action, "TuiRunner::handle_tick: pending action");
            info!("Executing action: {:?}", action);
            self.execute_action(action).await;
        }

        // Check for state change events (instant refresh on new executions - same process)
        self.process_state_events().await?;

        // Check for cross-process state changes via notification file
        let current_version = read_state_version();
        if current_version != self.last_state_version {
            debug!(
                old = self.last_state_version,
                new = current_version,
                "TuiRunner::handle_tick: state version changed, refreshing"
            );
            self.last_state_version = current_version;
            if self.state_manager.is_some() {
                self.refresh_data().await?;
                self.last_refresh = Instant::now();
            }
        }

        // Refresh data if interval has elapsed (fallback polling)
        if self.state_manager.is_some() && self.last_refresh.elapsed() >= DATA_REFRESH_INTERVAL {
            debug!("TuiRunner::handle_tick: refreshing data");
            self.refresh_data().await?;
            self.last_refresh = Instant::now();
        }

        Ok(())
    }

    /// Process pending stream chunks from LLM (non-blocking)
    fn process_stream_chunks(&mut self) {
        debug!("TuiRunner::process_stream_chunks: called");
        if let Some(rx) = &mut self.stream_rx {
            // Collect all chunks first to avoid borrow issues
            let chunks: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
            if !chunks.is_empty() {
                trace!("Processing {} stream chunks", chunks.len());
            }
            for chunk in chunks {
                match chunk {
                    StreamChunk::MessageStart { input_tokens } => {
                        trace!("Message started: {} input tokens", input_tokens);
                        self.app.state_mut().streaming_input_tokens = Some(input_tokens);
                    }
                    StreamChunk::TextDelta(text) => {
                        trace!("Received text delta: {} chars", text.len());
                        self.app.state_mut().repl_response_buffer.push_str(&text);
                    }
                    StreamChunk::ToolUseStart { ref name, .. } => {
                        debug!("Tool use started: {}", name);
                        self.app
                            .state_mut()
                            .repl_response_buffer
                            .push_str(&format!("\n[calling {}]", name));
                    }
                    StreamChunk::MessageDone { ref usage, .. } => {
                        trace!("Message done: {} output tokens", usage.output_tokens);
                        self.app.state_mut().streaming_output_tokens = Some(usage.output_tokens);
                    }
                    StreamChunk::Error(ref err) => {
                        warn!("Stream error: {}", err);
                        self.app.state_mut().repl_history.push(ReplMessage::error(err));
                    }
                    _ => {}
                }
            }
        }
    }

    /// Process LLM task results (non-blocking check)
    async fn process_llm_results(&mut self) {
        debug!("TuiRunner::process_llm_results: called");
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

        if !results.is_empty() {
            info!("Processing {} LLM result(s)", results.len());
        }

        // Now process each result
        for result in results {
            match result {
                LlmTaskResult::Response {
                    content,
                    tool_calls,
                    stop_reason,
                    input_tokens,
                    output_tokens,
                } => {
                    info!(
                        "LLM response: stop_reason={:?}, has_content={}, tool_calls={}, tokens={}in/{}out",
                        stop_reason,
                        content.is_some(),
                        tool_calls.len(),
                        input_tokens,
                        output_tokens
                    );
                    // Handle the response based on stop reason
                    match stop_reason {
                        StopReason::EndTurn => {
                            debug!("Response complete (EndTurn)");
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

                                // Log assistant message
                                self.conversation_logger.log_assistant_message(&response_text);
                            }
                            // Update session totals
                            self.app.state_mut().finish_request(input_tokens, output_tokens);
                            self.finish_streaming();
                        }
                        StopReason::ToolUse => {
                            info!("Response has tool calls, executing {} tool(s)", tool_calls.len());
                            // Update session totals (before continuing with tool calls)
                            self.app.state_mut().finish_request(input_tokens, output_tokens);
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
                            // Update session totals
                            self.app.state_mut().finish_request(input_tokens, output_tokens);
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
                            // Update session totals
                            self.app.state_mut().finish_request(input_tokens, output_tokens);
                            self.finish_streaming();
                        }
                    }
                }
                LlmTaskResult::Error(err) => {
                    warn!("LLM task error: {}", err);
                    self.app
                        .state_mut()
                        .repl_history
                        .push(ReplMessage::error(format!("LLM error: {}", err)));

                    // Log error
                    self.conversation_logger.log_error(&err);
                    self.finish_streaming();
                }
            }
        }
    }

    /// Finish streaming state
    fn finish_streaming(&mut self) {
        debug!("TuiRunner::finish_streaming: called");
        info!("Finishing streaming, setting repl_streaming=false");
        self.app.state_mut().repl_streaming = false;
        self.app.state_mut().repl_response_buffer.clear();
        self.stream_rx = None;
        self.llm_result_rx = None;
        self.llm_task = None;
    }

    /// Handle tool calls from LLM response
    async fn handle_tool_calls(&mut self, content: Option<String>, tool_calls: Vec<ToolCall>) {
        debug!(
            tool_count = tool_calls.len(),
            has_content = content.is_some(),
            "TuiRunner::handle_tool_calls: called"
        );
        info!("handle_tool_calls: {} tools to execute", tool_calls.len());

        if tool_calls.is_empty() {
            debug!("No tool calls, finishing streaming");
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
            debug!("Adding tool use to conversation: {} (id={})", tc.name, tc.id);
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
            info!("Executing tool: {} (id={})", tc.name, tc.id);
            // Format tool args for display
            let tool_args = Self::format_tool_args(&tc.input);

            // Show tool call with args
            self.app
                .state_mut()
                .repl_history
                .push(ReplMessage::tool_result_with_args(
                    &tc.name,
                    &tool_args,
                    format!("Running {}...", tc.name),
                ));

            // Log tool call
            let input_str = serde_json::to_string(&tc.input).unwrap_or_else(|_| "{}".to_string());
            self.conversation_logger.log_tool_call(&tc.name, &input_str);

            let result = self.tool_executor.execute(tc, &ctx).await;
            debug!(
                "Tool {} result: {} chars, is_error={}",
                tc.name,
                result.content.len(),
                result.is_error
            );

            // Log tool result
            self.conversation_logger.log_tool_result(&tc.name, &result.content);

            // Replace the "Running..." message with actual result
            self.app.state_mut().repl_history.pop();
            self.app
                .state_mut()
                .repl_history
                .push(ReplMessage::tool_result_with_args(&tc.name, tool_args, &result.content));

            result_blocks.push(ContentBlock::tool_result(&tc.id, &result.content, result.is_error));
        }

        info!(
            "All {} tools executed, adding results to conversation",
            tool_calls.len()
        );
        // Add tool results to conversation
        self.repl_conversation.push(Message::user_blocks(result_blocks));

        // Clear response buffer for next LLM turn
        self.app.state_mut().repl_response_buffer.clear();

        // Continue with another LLM call (streaming indicator provides visual feedback)
        self.continue_llm_request();
    }

    /// Start a new REPL request (spawns background task)
    fn start_repl_request(&mut self, input: &str) {
        debug!(input_len = input.len(), "TuiRunner::start_repl_request: called");
        info!(
            "start_repl_request: mode={:?}, input_len={}, streaming={}",
            self.app.state().repl_mode,
            input.len(),
            self.app.state().repl_streaming
        );

        // Don't start a new request if we're already streaming
        if self.app.state().repl_streaming {
            warn!("Blocked: already streaming, cannot start new request");
            self.app
                .state_mut()
                .repl_history
                .push(ReplMessage::error("Please wait for the current response to complete."));
            return;
        }

        // Check if we have an LLM client
        let llm = match &self.llm_client {
            Some(llm) => Arc::clone(llm),
            None => {
                warn!("No LLM client configured");
                self.app.state_mut().repl_history.push(ReplMessage::error(
                    "No LLM client configured. Check your config's api-key-env or api-key-file.",
                ));
                return;
            }
        };

        // Add user message to display history
        self.app.state_mut().repl_history.push(ReplMessage::user(input));

        // Add user message to LLM conversation
        self.repl_conversation.push(Message::user(input));
        info!("Conversation now has {} messages", self.repl_conversation.len());

        // Log user message for debugging
        let mode = match self.app.state().repl_mode {
            ReplMode::Chat => "Chat",
            ReplMode::Plan => "Plan",
        };
        self.conversation_logger.set_mode(mode);
        self.conversation_logger.log_user_message(input);

        // Set streaming state with fun word and start time
        self.app.state_mut().repl_streaming = true;
        self.app.state_mut().repl_response_buffer.clear();
        self.app.state_mut().start_streaming("claude-sonnet-4"); // Default model for cost estimation

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
            max_tokens: self.max_tokens,
        };

        info!("Spawning LLM request task with {} tools", request.tools.len());

        // Spawn background task
        self.llm_task = Some(tokio::spawn(async move {
            debug!("LLM task started");
            let result = match llm.stream(request, stream_tx).await {
                Ok(response) => {
                    debug!("LLM stream completed successfully");
                    LlmTaskResult::Response {
                        content: response.content,
                        tool_calls: response.tool_calls,
                        stop_reason: response.stop_reason,
                        input_tokens: response.usage.input_tokens,
                        output_tokens: response.usage.output_tokens,
                    }
                }
                Err(e) => {
                    warn!("LLM stream failed: {}", e);
                    LlmTaskResult::Error(e.to_string())
                }
            };
            let _ = result_tx.send(result).await;
        }));
    }

    /// Continue LLM request after tool execution (spawns background task)
    fn continue_llm_request(&mut self) {
        debug!("TuiRunner::continue_llm_request: called");
        // Check if we have an LLM client
        let llm = match &self.llm_client {
            Some(llm) => {
                debug!("TuiRunner::continue_llm_request: LLM client found");
                Arc::clone(llm)
            }
            None => {
                debug!("TuiRunner::continue_llm_request: no LLM client, finishing");
                self.finish_streaming();
                return;
            }
        };

        debug!(
            "Continuing LLM request with {} messages in conversation",
            self.repl_conversation.len()
        );

        // Set streaming state with fun word and start time
        self.app.state_mut().repl_streaming = true;
        self.app.state_mut().repl_response_buffer.clear();
        self.app.state_mut().start_streaming("claude-sonnet-4"); // Default model for cost estimation

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
            max_tokens: self.max_tokens,
        };

        // Spawn background task with timeout
        self.llm_task = Some(tokio::spawn(async move {
            // 2 minute timeout for continued requests
            let result =
                match tokio::time::timeout(std::time::Duration::from_secs(120), llm.stream(request, stream_tx)).await {
                    Ok(Ok(response)) => {
                        debug!("Continued request completed successfully");
                        LlmTaskResult::Response {
                            content: response.content,
                            tool_calls: response.tool_calls,
                            stop_reason: response.stop_reason,
                            input_tokens: response.usage.input_tokens,
                            output_tokens: response.usage.output_tokens,
                        }
                    }
                    Ok(Err(e)) => {
                        warn!("Continued request failed: {}", e);
                        LlmTaskResult::Error(e.to_string())
                    }
                    Err(_) => {
                        warn!("Continued request timed out after 2 minutes");
                        LlmTaskResult::Error("Request timed out after 2 minutes".to_string())
                    }
                };
            let _ = result_tx.send(result).await;
        }));
    }

    /// Generate a short title from task description using LLM
    async fn generate_title(&self, task: &str) -> Option<String> {
        debug!(task_len = task.len(), "TuiRunner::generate_title: called");
        let llm = self.llm_client.as_ref()?;
        crate::llm::name_markdown(llm, task).await
    }

    /// Start a new plan loop for the given task
    async fn start_task(&mut self, task: &str) {
        debug!(%task, "TuiRunner::start_task: called");
        let state_manager = match &self.state_manager {
            Some(sm) => {
                debug!("TuiRunner::start_task: state manager found");
                sm
            }
            None => {
                debug!("TuiRunner::start_task: no state manager");
                self.app.state_mut().set_error("No state manager - cannot create loop");
                return;
            }
        };

        debug!("Starting plan loop: {}", task);

        // Generate a short title via LLM (used for both display and ID slug)
        let title = self
            .generate_title(task)
            .await
            .unwrap_or_else(|| "New Task".to_string());

        // Create a plan execution - use the short title for the ID slug
        let mut execution = crate::domain::LoopExecution::new("plan", &title);
        execution.set_title(title);
        // Store the original task in context
        execution.set_context(serde_json::json!({ "user-request": task }));
        // New plans start as draft - user must approve before they run
        execution.set_status(crate::domain::LoopExecutionStatus::Draft);

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

    /// Start plan creation in a background task (non-blocking)
    fn start_plan_creation(&mut self, request: PlanCreateRequest) {
        debug!(
            message_count = request.messages.len(),
            "TuiRunner::start_plan_creation: called"
        );
        // Validate we have required dependencies
        let state_manager = match &self.state_manager {
            Some(sm) => {
                debug!("TuiRunner::start_plan_creation: state manager found");
                sm.clone()
            }
            None => {
                debug!("TuiRunner::start_plan_creation: no state manager");
                warn!("No StateManager - cannot create plan");
                self.app.state_mut().set_error("No state manager - cannot create plan");
                return;
            }
        };

        let llm = match &self.llm_client {
            Some(llm) => {
                debug!("TuiRunner::start_plan_creation: LLM client found");
                Arc::clone(llm)
            }
            None => {
                debug!("TuiRunner::start_plan_creation: no LLM client");
                warn!("No LLM client - cannot create plan");
                self.app.state_mut().set_error("No LLM client - cannot create plan");
                return;
            }
        };

        // Mark plan creation as in progress
        self.app.state_mut().plan_creating = true;

        // Show initial message
        self.app.state_mut().repl_history.push(ReplMessage::assistant(
            "Starting plan creation (Rule of Five)...".to_string(),
        ));

        // Create channel for progress updates
        let (progress_tx, progress_rx) = mpsc::channel::<PlanProgress>(100);
        self.plan_progress_rx = Some(progress_rx);

        // Clone what we need for the background task
        let worktree = self.worktree.clone();
        let max_tokens = self.max_tokens;

        // Spawn background task
        self.plan_task = Some(tokio::spawn(async move {
            Self::run_plan_creation(request, llm, state_manager, worktree, max_tokens, progress_tx).await;
        }));

        info!("Plan creation background task spawned");
    }

    /// Process plan creation progress messages (non-blocking)
    async fn process_plan_progress(&mut self) {
        debug!("TuiRunner::process_plan_progress: called");
        let progress_messages: Vec<PlanProgress> = if let Some(rx) = &mut self.plan_progress_rx {
            let mut collected = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                collected.push(msg);
            }
            collected
        } else {
            return;
        };

        for progress in progress_messages {
            match progress {
                PlanProgress::Started => {
                    // Add initial streaming message that will be appended to
                    self.app
                        .state_mut()
                        .repl_history
                        .push(ReplMessage::assistant(String::new()));
                }
                PlanProgress::TextChunk(chunk) => {
                    // Append chunk to the last message (streaming output)
                    if let Some(last) = self.app.state_mut().repl_history.last_mut() {
                        last.content.push_str(&chunk);
                    }
                }
                PlanProgress::Completed {
                    exec_id,
                    title,
                    plan_content: _,
                } => {
                    info!("Plan creation completed: {} / {}", exec_id, title);
                    self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                        "\n\n---\nPlan created: {} ({})\nView in Executions tab (Tab to switch).",
                        title, exec_id
                    )));
                    // Force data refresh to show the new draft
                    self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
                    // Clear plan creating flag
                    self.app.state_mut().plan_creating = false;
                    self.plan_progress_rx = None;
                    self.plan_task = None;
                }
                PlanProgress::Failed { error } => {
                    warn!("Plan creation failed: {}", error);
                    self.app
                        .state_mut()
                        .set_error(format!("Plan creation failed: {}", error));
                    self.app.state_mut().plan_creating = false;
                    self.plan_progress_rx = None;
                    self.plan_task = None;
                }
            }
        }
    }

    /// Process state change events for instant TUI updates
    ///
    /// When cascade creates child executions, the TUI is notified immediately
    /// and refreshes data without waiting for the polling interval.
    async fn process_state_events(&mut self) -> Result<()> {
        if let Some(rx) = &mut self.state_event_rx {
            // Drain all pending events
            let mut should_refresh = false;
            loop {
                match rx.try_recv() {
                    Ok(event) => match &event {
                        StateEvent::ExecutionCreated { id, loop_type } => {
                            debug!(
                                %id,
                                %loop_type,
                                "process_state_events: new execution created"
                            );
                            info!("New {} execution created: {}", loop_type, id);
                            should_refresh = true;
                        }
                        StateEvent::ExecutionUpdated { id } => {
                            debug!(%id, "process_state_events: execution updated");
                            should_refresh = true;
                        }
                        StateEvent::ExecutionPending { id } => {
                            debug!(%id, "process_state_events: execution pending (for LoopManager)");
                            // TUI doesn't need to do anything special for pending events,
                            // but we refresh to show the status change
                            should_refresh = true;
                        }
                    },
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                        debug!(n, "process_state_events: lagged behind, catching up");
                        should_refresh = true;
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                        debug!("process_state_events: channel closed");
                        self.state_event_rx = None;
                        break;
                    }
                }
            }

            // Refresh immediately if we received any events
            if should_refresh {
                debug!("process_state_events: triggering immediate refresh");
                self.refresh_data().await?;
                self.last_refresh = Instant::now();
            }
        }
        Ok(())
    }

    /// Run plan creation (executes in background task)
    /// Uses consolidated Rule of Five prompt - LLM self-reviews in a single call
    async fn run_plan_creation(
        request: PlanCreateRequest,
        llm: Arc<dyn LlmClient>,
        state_manager: StateManager,
        worktree: PathBuf,
        max_tokens: u32,
        progress_tx: mpsc::Sender<PlanProgress>,
    ) {
        debug!(
            message_count = request.messages.len(),
            "TuiRunner::run_plan_creation: called"
        );
        info!("=== run_plan_creation START (Rule of Five) ===");

        // Signal that plan creation has started
        let _ = progress_tx.send(PlanProgress::Started).await;

        // Format conversation for the LLM
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

        // Load the consolidated plan prompt (includes Rule of Five instructions)
        let plan_prompt = crate::prompts::embedded::get_embedded("plan")
            .unwrap_or("Create a plan document for this task.")
            .to_string();

        // Generate a short title
        info!("Generating title from conversation...");
        let title = Self::generate_title_static(&llm, &conversation_text)
            .await
            .unwrap_or_else(|| "New Plan".to_string());
        info!("Generated title: {}", title);

        // Build the LLM request - single call with consolidated prompt
        let user_message = format!("{}\n\n---\n\n# Conversation:\n\n{}", plan_prompt, conversation_text);

        let completion_request = CompletionRequest {
            system_prompt: String::new(), // Instructions are in the user message
            messages: vec![Message::user(&user_message)],
            tools: vec![],
            max_tokens,
        };

        // Create channel for streaming chunks
        let (chunk_tx, mut chunk_rx) = mpsc::channel::<crate::llm::StreamChunk>(100);

        // Clone progress_tx for the streaming forwarder
        let progress_tx_clone = progress_tx.clone();

        // Spawn task to forward stream chunks to progress channel
        let forward_task = tokio::spawn(async move {
            while let Some(chunk) = chunk_rx.recv().await {
                if let crate::llm::StreamChunk::TextDelta(text) = chunk {
                    let _ = progress_tx_clone.send(PlanProgress::TextChunk(text)).await;
                }
            }
        });

        // Execute the plan creation with streaming
        info!("Sending plan creation request to LLM (streaming)...");
        let plan_output = match llm.stream(completion_request, chunk_tx).await {
            Ok(response) => {
                let output = response.content.unwrap_or_default();
                info!("Plan creation complete: {} chars", output.len());
                output
            }
            Err(e) => {
                warn!("Plan creation failed: {}", e);
                let _ = progress_tx
                    .send(PlanProgress::Failed {
                        error: format!("Plan creation failed: {}", e),
                    })
                    .await;
                return;
            }
        };

        // Wait for forwarding to complete
        let _ = forward_task.await;

        // Extract just the final plan (after "=== FINAL PLAN ===" marker)
        let final_plan = Self::extract_final_plan(&plan_output);
        info!("Extracted final plan: {} chars", final_plan.len());

        // Create the plan execution with Draft status
        let mut execution = crate::domain::LoopExecution::new("plan", &title);
        execution.set_title(title.clone());
        execution.set_status(crate::domain::LoopExecutionStatus::Draft);

        execution.set_context(serde_json::json!({
            "user-request": conversation_text
        }));

        // Create the execution record
        let exec_id = match state_manager.create_execution(execution).await {
            Ok(id) => {
                info!("Draft execution created: {}", id);
                id
            }
            Err(e) => {
                warn!("Failed to create draft execution: {}", e);
                let _ = progress_tx
                    .send(PlanProgress::Failed {
                        error: format!("Failed to create draft: {}", e),
                    })
                    .await;
                return;
            }
        };

        // Create the plan directory and file
        let plan_dir = worktree.join(".taskdaemon/plans").join(&exec_id);
        if let Err(e) = std::fs::create_dir_all(&plan_dir) {
            warn!("Failed to create plan directory: {}", e);
            let _ = progress_tx
                .send(PlanProgress::Failed {
                    error: format!("Failed to create plan directory: {}", e),
                })
                .await;
            return;
        }

        // Write the final plan.md file (just the extracted plan, not review passes)
        let plan_content = format!(
            r#"# Plan: {}

**Status:** Draft
**Created:** {}
**ID:** {}

---

{}"#,
            title,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            exec_id,
            final_plan
        );

        let plan_path = plan_dir.join("plan.md");
        if let Err(e) = std::fs::write(&plan_path, &plan_content) {
            warn!("Failed to write plan file: {}", e);
            let _ = progress_tx
                .send(PlanProgress::Failed {
                    error: format!("Failed to write plan file: {}", e),
                })
                .await;
            return;
        }

        info!("Plan written to: {:?}", plan_path);

        // Send completion
        let _ = progress_tx
            .send(PlanProgress::Completed {
                exec_id,
                title,
                plan_content,
            })
            .await;
    }

    /// Format tool arguments for display (compact form)
    fn format_tool_args(input: &serde_json::Value) -> String {
        debug!("TuiRunner::format_tool_args: called");
        if let Some(obj) = input.as_object() {
            let parts: Vec<String> = obj
                .iter()
                .filter_map(|(k, v)| {
                    let val_str = match v {
                        serde_json::Value::String(s) => {
                            // Truncate long strings
                            if s.len() > 40 { format!("\"{}...\"", &s[..37]) } else { format!("\"{}\"", s) }
                        }
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        _ => return None, // Skip complex values
                    };
                    Some(format!("{}: {}", k, val_str))
                })
                .collect();
            parts.join(", ")
        } else {
            String::new()
        }
    }

    /// Extract the final plan from LLM output (content after "=== FINAL PLAN ===" marker)
    fn extract_final_plan(llm_output: &str) -> String {
        debug!(output_len = llm_output.len(), "TuiRunner::extract_final_plan: called");
        // Look for the final plan marker
        const MARKER: &str = "=== FINAL PLAN ===";

        if let Some(pos) = llm_output.find(MARKER) {
            debug!("TuiRunner::extract_final_plan: marker found at pos {}", pos);
            // Extract everything after the marker
            let after_marker = &llm_output[pos + MARKER.len()..];
            after_marker.trim().to_string()
        } else {
            // Fallback: if no marker found, use the entire output
            // This handles cases where LLM didn't follow the format
            debug!("TuiRunner::extract_final_plan: marker not found, using full output");
            warn!("No '=== FINAL PLAN ===' marker found, using full output");
            llm_output.trim().to_string()
        }
    }

    /// Generate title from conversation (static version for background task)
    async fn generate_title_static(llm: &Arc<dyn LlmClient>, conversation: &str) -> Option<String> {
        debug!(
            conversation_len = conversation.len(),
            "TuiRunner::generate_title_static: called"
        );
        // Load title generator prompt from embedded or file
        let system_prompt = crate::prompts::embedded::get_embedded("title")
            .unwrap_or("Generate a 2-5 word title. Output ONLY the title.")
            .to_string();

        let request = CompletionRequest {
            system_prompt,
            messages: vec![Message::user(conversation)],
            tools: vec![],
            max_tokens: 50,
        };

        match llm.complete(request).await {
            Ok(response) => response.content.map(|s| Self::clean_title(s.trim())),
            Err(_) => None,
        }
    }

    /// Clean and validate a generated title (delegates to standalone function)
    fn clean_title(raw: &str) -> String {
        debug!(%raw, "TuiRunner::clean_title: called");
        clean_title(raw)
    }

    /// Check if plan changed significantly (static version)
    fn plan_changed_significantly_static(old: &str, new: &str) -> bool {
        debug!(
            old_len = old.len(),
            new_len = new.len(),
            "TuiRunner::plan_changed_significantly_static: called"
        );
        if old.is_empty() {
            debug!("TuiRunner::plan_changed_significantly_static: old is empty");
            return true;
        }
        // Simple heuristic: check if length changed by more than 5%
        let old_len = old.len();
        let new_len = new.len();
        let diff = (new_len as i64 - old_len as i64).unsigned_abs() as usize;
        diff > old_len / 20 // More than 5% change
    }

    /// Check if the plan changed significantly between passes
    ///
    /// Returns true if there are meaningful changes, false if the plans are essentially the same.
    /// This is used to detect convergence in the Rule of Five iteration.
    fn plan_changed_significantly(&self, old_plan: &str, new_plan: &str) -> bool {
        debug!(
            old_len = old_plan.len(),
            new_len = new_plan.len(),
            "TuiRunner::plan_changed_significantly: called"
        );
        if old_plan.is_empty() {
            debug!("TuiRunner::plan_changed_significantly: old plan is empty, first pass");
            return true; // First pass always counts as a change
        }

        // Normalize whitespace for comparison
        let normalize = |s: &str| -> String {
            s.lines()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        };

        let old_normalized = normalize(old_plan);
        let new_normalized = normalize(new_plan);

        // Simple length-based heuristic: if the length changed by more than 5%, it's significant
        let len_old = old_normalized.len();
        let len_new = new_normalized.len();
        let len_diff = (len_new as i64 - len_old as i64).unsigned_abs() as usize;
        let threshold = (len_old.max(len_new) as f64 * 0.05) as usize;

        if len_diff > threshold {
            return true;
        }

        // If lengths are similar, check for actual content changes
        old_normalized != new_normalized
    }

    /// Execute a pending action (cancel/pause/resume/start draft)
    async fn execute_action(&mut self, action: PendingAction) {
        debug!(?action, "TuiRunner::execute_action: called");
        let state_manager = match &self.state_manager {
            Some(sm) => {
                debug!("TuiRunner::execute_action: state manager found");
                sm
            }
            None => {
                debug!("TuiRunner::execute_action: no state manager");
                self.app
                    .state_mut()
                    .set_error("No state manager - cannot execute action");
                return;
            }
        };

        match action {
            PendingAction::CancelLoop(id) => {
                debug!(%id, "TuiRunner::execute_action: CancelLoop");
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
                        self.app.state_mut().describe_data = None; // Force reload
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
                        self.app.state_mut().describe_data = None; // Force reload
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
            PendingAction::ActivateDraft(id) => {
                debug!("Activating draft: {}", id);
                match state_manager.activate_draft(&id).await {
                    Ok(()) => {
                        debug!("Activated draft {} (now pending, awaiting LoopManager pickup)", id);
                        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
                        self.app.state_mut().describe_data = None; // Force reload
                    }
                    Err(e) => {
                        warn!("Failed to activate draft: {}", e);
                        self.app.state_mut().set_error(format!("Failed to activate: {}", e));
                    }
                }
            }
        }
    }

    /// Handle key event
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        debug!(?key, "TuiRunner::handle_key: called");
        self.app.handle_key(key)
    }

    /// Handle mouse event
    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        debug!(?mouse, "TuiRunner::handle_mouse: called");
        use crossterm::event::MouseEventKind;

        let state = self.app.state_mut();

        // Only handle scroll in REPL view
        if !matches!(state.current_view, View::Repl) {
            debug!("TuiRunner::handle_mouse: not in REPL view, ignoring");
            return;
        }

        let max = state.repl_max_scroll;
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                debug!("TuiRunner::handle_mouse: ScrollUp");
                state.repl_scroll_up(3, max);
            }
            MouseEventKind::ScrollDown => {
                debug!("TuiRunner::handle_mouse: ScrollDown");
                state.repl_scroll_down(3, max);
            }
            _ => {
                debug!("TuiRunner::handle_mouse: other event, not handled");
                // Other mouse events (click, drag) not handled yet
            }
        }
    }

    /// Handle terminal resize
    fn handle_resize(&mut self, width: u16, height: u16) {
        debug!(width, height, "TuiRunner::handle_resize: called");
        // Terminal handles resize automatically, but we might want to adjust state
        debug!("Terminal resized");
    }

    /// Refresh data from StateManager
    async fn refresh_data(&mut self) -> Result<()> {
        debug!("TuiRunner::refresh_data: called");

        // Check daemon status
        let daemon = DaemonManager::new();
        let daemon_status = if daemon.is_running() {
            if daemon.version_matches() {
                DaemonStatus::Connected
            } else {
                DaemonStatus::VersionMismatch
            }
        } else {
            DaemonStatus::Disconnected
        };
        self.app.state_mut().daemon_status = daemon_status;

        let state_manager = match &self.state_manager {
            Some(sm) => {
                debug!("TuiRunner::refresh_data: state manager found");
                sm
            }
            None => {
                debug!("TuiRunner::refresh_data: no state manager, returning");
                return Ok(());
            }
        };

        // Sync Loop records and build exec_id -> artifact map
        let mut artifacts_by_exec_id: std::collections::HashMap<String, RecordItem> = std::collections::HashMap::new();

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

                        // Extract exec_id from context if present
                        let exec_id = l.context.get("exec_id").and_then(|v| v.as_str()).map(|s| s.to_string());

                        RecordItem {
                            id: l.id.clone(),
                            title: l.title.clone(),
                            loop_type: l.r#type.clone(),
                            status: l.status.to_string(),
                            parent_id: l.parent.clone(),
                            children_count: 0, // TODO: count children
                            phases_progress,
                            created: format_time_ago(l.created_at),
                            exec_id,
                            file: l.file.clone(),
                        }
                    })
                    .collect();

                // Build map for linking artifacts to executions
                for item in &items {
                    if let Some(ref exec_id) = item.exec_id {
                        artifacts_by_exec_id.insert(exec_id.clone(), item.clone());
                    }
                }

                let state = self.app.state_mut();
                state.total_records = items.len();
                state.records = items;
            }
            Err(e) => {
                warn!("Failed to fetch loops: {}", e);
            }
        }

        // Sync loop executions and link to artifacts
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

                        // Look up associated artifact (Loop record)
                        let artifact = artifacts_by_exec_id.get(&e.id);

                        ExecutionItem {
                            id: e.id.clone(),
                            name,
                            loop_type: e.loop_type.clone(),
                            iteration: format!("{}/10", e.iteration), // TODO: get max from config
                            status: e.status.to_string(),
                            duration,
                            parent_id: e.parent.clone(),
                            progress,
                            artifact_id: artifact.map(|a| a.id.clone()),
                            artifact_file: artifact.and_then(|a| a.file.clone()),
                            artifact_status: artifact.map(|a| a.status.clone()),
                        }
                    })
                    .collect();

                let state = self.app.state_mut();
                state.executions_draft = items.iter().filter(|r| r.status == "draft").count();
                state.executions_active = items.iter().filter(|r| r.status == "running").count();
                state.executions_complete = items.iter().filter(|r| r.status == "complete").count();
                state.executions_failed = items.iter().filter(|r| r.status == "failed").count();
                state.executions = items.clone();
                // Always build the loops tree so it's ready when user switches to Loops view
                state.loops_tree.build_from_items(items);
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
        debug!("TuiRunner::load_view_data: called");
        let state_manager = match &self.state_manager {
            Some(sm) => {
                debug!("TuiRunner::load_view_data: state manager found");
                sm
            }
            None => {
                debug!("TuiRunner::load_view_data: no state manager, returning");
                return Ok(());
            }
        };

        let view = self.app.state().current_view.clone();
        debug!(?view, "TuiRunner::load_view_data: current view");

        match view {
            View::Logs { ref target_id } => {
                debug!(%target_id, "TuiRunner::load_view_data: loading logs");
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

                    // Load plan content from disk if it exists
                    let plan_path = self.worktree.join(".taskdaemon/plans").join(&exec.id).join("plan.md");
                    let plan_content = std::fs::read_to_string(&plan_path).ok();

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
                        plan_content,
                        output: if exec.progress.is_empty() { None } else { Some(exec.progress.clone()) },
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
                        plan_content: None, // Loop records don't have plan content
                        output: None,       // Loop records don't have output
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

/// Format a timestamp as ISO date string in local timezone
fn format_timestamp(timestamp_ms: i64) -> String {
    use chrono::{Local, TimeZone};
    let dt = Local.timestamp_millis_opt(timestamp_ms);
    match dt {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => "unknown".to_string(),
    }
}

/// Clean and validate a generated title, ensuring it's short enough for use as a slug
fn clean_title(raw: &str) -> String {
    // Remove quotes if present
    let cleaned = raw.trim_matches('"').trim_matches('\'').trim();

    // If the "title" looks like a conversational response (starts with I'll, Here's, etc.),
    // extract just the key noun phrase
    let title = if cleaned.to_lowercase().starts_with("i'll ")
        || cleaned.to_lowercase().starts_with("i will ")
        || cleaned.to_lowercase().starts_with("here's ")
        || cleaned.to_lowercase().starts_with("this is ")
    {
        // Try to extract key words - take the last few meaningful words
        let words: Vec<&str> = cleaned.split_whitespace().collect();
        if words.len() > 5 {
            // Look for key nouns after common verbs
            words.iter().rev().take(4).rev().cloned().collect::<Vec<_>>().join(" ")
        } else {
            cleaned.to_string()
        }
    } else {
        cleaned.to_string()
    };

    // Limit to 5 words max
    let words: Vec<&str> = title.split_whitespace().take(5).collect();
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_refresh_interval() {
        assert_eq!(DATA_REFRESH_INTERVAL, Duration::from_millis(250));
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

    // === TESTS: clean_title ===

    #[test]
    fn test_clean_title_removes_quotes() {
        assert_eq!(clean_title("\"Hello World\""), "Hello World");
        assert_eq!(clean_title("'Some Title'"), "Some Title");
    }

    #[test]
    fn test_clean_title_limits_to_five_words() {
        let long = "One Two Three Four Five Six Seven";
        assert_eq!(clean_title(long), "One Two Three Four Five");
    }

    #[test]
    fn test_clean_title_handles_conversational_response() {
        // "I'll create a Rust CLI project with a greeting feature" -> last 4 words
        let conv = "I'll create a Rust CLI project with a greeting feature";
        let result = clean_title(conv);
        // Should extract last 4 words: "with a greeting feature"
        assert_eq!(result, "with a greeting feature");
    }

    #[test]
    fn test_clean_title_handles_short_conversational() {
        // Short "I'll" response stays as-is (5 words or less)
        let short = "I'll do that";
        assert_eq!(clean_title(short), "I'll do that");
    }

    #[test]
    fn test_clean_title_preserves_normal_title() {
        assert_eq!(clean_title("OAuth Authentication"), "OAuth Authentication");
        assert_eq!(clean_title("Howdy CLI"), "Howdy CLI");
    }

    #[test]
    fn test_clean_title_handles_heres_prefix() {
        let heres = "Here's the complete specification for your project";
        let result = clean_title(heres);
        assert_eq!(result, "specification for your project");
    }
}
