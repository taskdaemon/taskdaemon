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
use crate::prompts::{FocusArea, PromptContext, PromptLoader};
use crate::state::StateManager;
use crate::tools::{ToolContext, ToolExecutor};

use super::Tui;
use super::app::App;
use super::conversation_log::ConversationLogger;
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
    /// Conversation logger for debug mode
    conversation_logger: ConversationLogger,

    // === Plan creation state ===
    /// Receiver for plan creation progress messages
    plan_progress_rx: Option<mpsc::Receiver<PlanProgress>>,
    /// Handle to the background plan creation task
    plan_task: Option<JoinHandle<()>>,
}

/// Progress updates from plan creation background task
#[derive(Debug)]
enum PlanProgress {
    /// Started a new pass
    PassStarted { pass: u8, focus: String },
    /// Pass completed with the plan output from this pass
    PassCompleted { pass: u8, plan_output: String },
    /// Plan creation completed successfully
    Completed {
        exec_id: String,
        title: String,
        plan_content: String,
        passes_run: u8,
    },
    /// Plan creation failed
    Failed { error: String },
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
            conversation_logger: ConversationLogger::disabled(),
            plan_progress_rx: None,
            plan_task: None,
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
            conversation_logger: ConversationLogger::disabled(),
            plan_progress_rx: None,
            plan_task: None,
        }
    }

    /// Create a new TuiRunner with LLM client for REPL
    pub fn with_llm_client(
        terminal: Tui,
        state_manager: Option<StateManager>,
        llm_client: Arc<dyn LlmClient>,
        log_conversations: bool,
    ) -> Self {
        let worktree = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let chat_system_prompt = Self::build_chat_system_prompt(&worktree);
        let plan_system_prompt = Self::build_plan_system_prompt(&worktree);

        let conversation_logger = if log_conversations {
            ConversationLogger::enabled()
        } else {
            ConversationLogger::disabled()
        };

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
            conversation_logger,
            plan_progress_rx: None,
            plan_task: None,
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
            // Process stream chunks for immediate display
            self.process_stream_chunks();

            // Draw the UI
            self.terminal.draw(|frame| views::render(self.app.state_mut(), frame))?;

            // Wait for either an event OR a plan progress message
            // This ensures plan progress updates trigger immediate redraws
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
                        // Return a pending future that never completes
                        std::future::pending::<Option<PlanProgress>>().await
                    }
                } => {
                    self.handle_plan_progress(progress);
                }
            }

            // Check if we should quit
            if self.app.state().should_quit {
                break;
            }
        }

        Ok(())
    }

    /// Handle a single plan progress message
    fn handle_plan_progress(&mut self, progress: PlanProgress) {
        match progress {
            PlanProgress::PassStarted { pass, focus } => {
                self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                    "  Pass {}/5: {} review...",
                    pass, focus
                )));
            }
            PlanProgress::PassCompleted { pass, plan_output } => {
                debug!("Pass {} completed", pass);
                // Show the plan output from this pass in the chat
                self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                    "  Pass {}/5 output:\n\n{}",
                    pass, plan_output
                )));
            }
            PlanProgress::Completed {
                exec_id,
                title,
                plan_content: _,
                passes_run,
            } => {
                info!(
                    "Plan creation completed: {} / {} ({} passes)",
                    exec_id, title, passes_run
                );
                self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                    "Plan created: {} ({})\n{} review passes completed.\nView in Executions tab (Tab to switch).",
                    title, exec_id, passes_run
                )));
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

    /// Handle tick event - periodic updates
    async fn handle_tick(&mut self) -> Result<()> {
        self.app.state_mut().tick();

        // Check for pending REPL submit - spawn background task
        if let Some(input) = self.app.state_mut().pending_repl_submit.take() {
            info!("REPL submit received: {} chars", input.len());
            self.start_repl_request(&input);
        }

        // Process streaming chunks if we're streaming
        self.process_stream_chunks();

        // Check for LLM task results
        self.process_llm_results().await;

        // Check for pending task to start
        if let Some(task) = self.app.state_mut().pending_task.take() {
            info!("Starting task: {}", task);
            self.start_task(&task).await;
        }

        // Check for pending plan creation - spawn background task
        if let Some(request) = self.app.state_mut().pending_plan_create.take() {
            info!("Creating plan from {} messages", request.messages.len());
            self.start_plan_creation(request);
        }

        // Process plan creation progress
        self.process_plan_progress().await;

        // Check for pending action (cancel/pause/resume/start draft)
        if let Some(action) = self.app.state_mut().pending_action.take() {
            info!("Executing action: {:?}", action);
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
            let mut chunk_count = 0;
            while let Ok(chunk) = rx.try_recv() {
                chunk_count += 1;
                match chunk {
                    StreamChunk::TextDelta(text) => {
                        trace!("Received text delta: {} chars", text.len());
                        self.app.state_mut().repl_response_buffer.push_str(&text);
                    }
                    StreamChunk::ToolUseStart { ref name, .. } => {
                        debug!("Tool use started: {}", name);
                        // Show tool call in response buffer
                        self.app
                            .state_mut()
                            .repl_response_buffer
                            .push_str(&format!("\n[calling {}]", name));
                    }
                    StreamChunk::Error(ref err) => {
                        warn!("Stream error: {}", err);
                        self.app.state_mut().repl_history.push(ReplMessage::error(err));
                    }
                    _ => {}
                }
            }
            if chunk_count > 0 {
                trace!("Processed {} stream chunks", chunk_count);
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
                } => {
                    info!(
                        "LLM response: stop_reason={:?}, has_content={}, tool_calls={}",
                        stop_reason,
                        content.is_some(),
                        tool_calls.len()
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
                            self.finish_streaming();
                        }
                        StopReason::ToolUse => {
                            info!("Response has tool calls, executing {} tool(s)", tool_calls.len());
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
        info!("Finishing streaming, setting repl_streaming=false");
        self.app.state_mut().repl_streaming = false;
        self.app.state_mut().repl_response_buffer.clear();
        self.stream_rx = None;
        self.llm_result_rx = None;
        self.llm_task = None;
    }

    /// Handle tool calls from LLM response
    async fn handle_tool_calls(&mut self, content: Option<String>, tool_calls: Vec<ToolCall>) {
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
            // Show tool call
            self.app
                .state_mut()
                .repl_history
                .push(ReplMessage::tool_result(&tc.name, format!("Running {}...", tc.name)));

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

        info!(
            "All {} tools executed, adding results to conversation",
            tool_calls.len()
        );
        // Add tool results to conversation
        self.repl_conversation.push(Message::user_blocks(result_blocks));

        // Clear response buffer for next LLM turn
        self.app.state_mut().repl_response_buffer.clear();

        // Show that we're continuing (visual feedback)
        self.app
            .state_mut()
            .repl_history
            .push(ReplMessage::assistant("Analyzing results..."));

        // Continue with another LLM call
        self.continue_llm_request();
    }

    /// Start a new REPL request (spawns background task)
    fn start_repl_request(&mut self, input: &str) {
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
                    "No LLM client configured. Start with daemon or set ANTHROPIC_API_KEY.",
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
        // Check if we have an LLM client
        let llm = match &self.llm_client {
            Some(llm) => Arc::clone(llm),
            None => {
                self.finish_streaming();
                return;
            }
        };

        debug!(
            "Continuing LLM request with {} messages in conversation",
            self.repl_conversation.len()
        );

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
        let llm = self.llm_client.as_ref()?;

        // Load title generator prompt from embedded or file
        let system_prompt = crate::prompts::embedded::get_embedded("title")
            .unwrap_or("Generate a 2-5 word title. Output ONLY the title.")
            .to_string();

        let request = CompletionRequest {
            system_prompt,
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

    /// Start plan creation in a background task (non-blocking)
    fn start_plan_creation(&mut self, request: PlanCreateRequest) {
        // Validate we have required dependencies
        let state_manager = match &self.state_manager {
            Some(sm) => sm.clone(),
            None => {
                warn!("No StateManager - cannot create plan");
                self.app.state_mut().set_error("No state manager - cannot create plan");
                return;
            }
        };

        let llm = match &self.llm_client {
            Some(llm) => Arc::clone(llm),
            None => {
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

        // Spawn background task
        self.plan_task = Some(tokio::spawn(async move {
            Self::run_plan_creation(request, llm, state_manager, worktree, progress_tx).await;
        }));

        info!("Plan creation background task spawned");
    }

    /// Process plan creation progress messages (non-blocking)
    async fn process_plan_progress(&mut self) {
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
                PlanProgress::PassStarted { pass, focus } => {
                    self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                        "  Pass {}/5: {} review...",
                        pass, focus
                    )));
                }
                PlanProgress::PassCompleted { pass, plan_output } => {
                    debug!("Pass {} completed", pass);
                    // Show the plan output from this pass in the chat
                    self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                        "  Pass {}/5 output:\n\n{}",
                        pass, plan_output
                    )));
                }
                PlanProgress::Completed {
                    exec_id,
                    title,
                    plan_content: _,
                    passes_run,
                } => {
                    info!(
                        "Plan creation completed: {} / {} ({} passes)",
                        exec_id, title, passes_run
                    );
                    self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                        "Plan created: {} ({})\n{} review passes completed.\nView in Executions tab (Tab to switch).",
                        title, exec_id, passes_run
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

    /// Run plan creation (executes in background task)
    async fn run_plan_creation(
        request: PlanCreateRequest,
        llm: Arc<dyn LlmClient>,
        state_manager: StateManager,
        worktree: PathBuf,
        progress_tx: mpsc::Sender<PlanProgress>,
    ) {
        info!("=== run_plan_creation START (Rule of Five) ===");

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

        // Load prompt templates
        let prompt_loader = PromptLoader::new(&worktree);

        // Get system prompt for plan creation
        let system_prompt = match prompt_loader.system_prompt() {
            Ok(prompt) => prompt,
            Err(e) => {
                warn!("Failed to load plan system prompt: {}", e);
                let _ = progress_tx
                    .send(PlanProgress::Failed {
                        error: "Failed to load plan prompts".to_string(),
                    })
                    .await;
                return;
            }
        };

        // Generate a short title
        info!("Generating title from conversation...");
        let title = Self::generate_title_static(&llm, &conversation_text)
            .await
            .unwrap_or_else(|| "New Plan".to_string());
        info!("Generated title: {}", title);

        // === RULE OF FIVE ITERATION ===
        let mut current_plan = String::new();
        let mut pass_outputs: Vec<String> = Vec::new();
        let mut consecutive_unchanged = 0;
        const MAX_PASSES: u8 = 5;
        const MIN_PASSES: u8 = 3;

        for pass_num in 1..=MAX_PASSES {
            let focus = match FocusArea::from_pass(pass_num) {
                Some(f) => f,
                None => break,
            };

            info!("=== Rule of Five Pass {}: {} ===", pass_num, focus);

            // Send progress update
            let _ = progress_tx
                .send(PlanProgress::PassStarted {
                    pass: pass_num,
                    focus: focus.name().to_string(),
                })
                .await;

            // Build context for this pass
            let ctx = if pass_num == 1 {
                PromptContext::first_pass(conversation_text.clone())
            } else {
                PromptContext::review_pass(pass_num, current_plan.clone(), focus)
            };

            // Get the pass-specific prompt
            let pass_prompt = match prompt_loader.pass_prompt(pass_num, &ctx) {
                Ok(p) => p,
                Err(e) => {
                    warn!("Failed to load pass {} prompt: {}", pass_num, e);
                    let _ = progress_tx
                        .send(PlanProgress::Failed {
                            error: format!("Failed to load pass {} prompt", pass_num),
                        })
                        .await;
                    return;
                }
            };

            // Build the LLM request
            let user_message = if pass_num == 1 {
                format!(
                    "{}\n\n---\n\n# Conversation to convert to Plan:\n\n{}",
                    pass_prompt, conversation_text
                )
            } else {
                format!("{}\n\n---\n\n# Current Plan:\n\n{}", pass_prompt, current_plan)
            };

            let completion_request = CompletionRequest {
                system_prompt: system_prompt.clone(),
                messages: vec![Message::user(&user_message)],
                tools: vec![],
                max_tokens: 8192,
            };

            // Execute the pass
            info!("Sending pass {} request to LLM...", pass_num);
            let pass_output = match llm.complete(completion_request).await {
                Ok(response) => {
                    let output = response.content.unwrap_or_default();
                    info!("Pass {} complete: {} chars", pass_num, output.len());
                    output
                }
                Err(e) => {
                    warn!("Pass {} failed: {}", pass_num, e);
                    let _ = progress_tx
                        .send(PlanProgress::Failed {
                            error: format!("Pass {} failed: {}", pass_num, e),
                        })
                        .await;
                    return;
                }
            };

            let _ = progress_tx
                .send(PlanProgress::PassCompleted {
                    pass: pass_num,
                    plan_output: pass_output.clone(),
                })
                .await;

            // Check for convergence
            let changed = Self::plan_changed_significantly_static(&current_plan, &pass_output);

            if changed {
                consecutive_unchanged = 0;
            } else {
                consecutive_unchanged += 1;
            }

            current_plan = pass_output.clone();
            pass_outputs.push(pass_output);

            // Check for early convergence
            if pass_num >= MIN_PASSES && consecutive_unchanged >= 2 {
                info!("Early convergence after pass {}", pass_num);
                break;
            }
        }

        info!("Rule of Five complete: {} passes executed", pass_outputs.len());

        // Create the plan execution with Draft status
        let mut execution = crate::domain::LoopExecution::new("plan", &title);
        execution.set_title(title.clone());
        execution.set_status(crate::domain::LoopExecutionStatus::Draft);

        execution.set_context(serde_json::json!({
            "user-request": conversation_text,
            "review-passes-completed": pass_outputs.len(),
            "final-plan": current_plan
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

        // Write the final plan.md file
        let plan_content = format!(
            r#"# Plan: {}

**Status:** Draft
**Created:** {}
**ID:** {}
**Review Passes:** {} of 5

---

{}"#,
            title,
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            exec_id,
            pass_outputs.len(),
            current_plan
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
                passes_run: pass_outputs.len() as u8,
            })
            .await;
    }

    /// Generate title from conversation (static version for background task)
    async fn generate_title_static(llm: &Arc<dyn LlmClient>, conversation: &str) -> Option<String> {
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
            Ok(response) => response.content.map(|s| s.trim().to_string()),
            Err(_) => None,
        }
    }

    /// Check if plan changed significantly (static version)
    fn plan_changed_significantly_static(old: &str, new: &str) -> bool {
        if old.is_empty() {
            return true;
        }
        // Simple heuristic: check if length changed by more than 5%
        let old_len = old.len();
        let new_len = new.len();
        let diff = (new_len as i64 - old_len as i64).unsigned_abs() as usize;
        diff > old_len / 20 // More than 5% change
    }

    /// Create a plan draft from conversation using the Rule of Five methodology
    /// (DEPRECATED - use start_plan_creation instead, this is kept for reference)
    #[allow(dead_code)]
    async fn create_plan_draft(&mut self, request: PlanCreateRequest) {
        info!("=== create_plan_draft START (Rule of Five) ===");
        info!(
            "StateManager present: {}, LLM client present: {}",
            self.state_manager.is_some(),
            self.llm_client.is_some()
        );

        let state_manager = match &self.state_manager {
            Some(sm) => {
                info!("StateManager connected");
                sm
            }
            None => {
                warn!("No StateManager - cannot create plan");
                self.app.state_mut().set_error("No state manager - cannot create plan");
                return;
            }
        };

        let llm = match &self.llm_client {
            Some(llm) => {
                info!("LLM client connected");
                Arc::clone(llm)
            }
            None => {
                warn!("No LLM client - cannot summarize conversation");
                self.app
                    .state_mut()
                    .set_error("No LLM client - cannot summarize conversation");
                return;
            }
        };

        info!("Creating plan draft from {} messages", request.messages.len());

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

        // Load prompt templates
        let prompt_loader = PromptLoader::new(&self.worktree);

        // Get system prompt for plan creation
        let system_prompt = match prompt_loader.system_prompt() {
            Ok(prompt) => prompt,
            Err(e) => {
                warn!("Failed to load plan system prompt: {}", e);
                self.app.state_mut().set_error("Failed to load plan prompts");
                return;
            }
        };

        // Generate a short title first
        info!("Generating title from conversation...");
        let title = self
            .generate_title(&conversation_text)
            .await
            .unwrap_or_else(|| "New Plan".to_string());
        info!("Generated title: {}", title);

        // Show initial progress message
        self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
            "Creating plan: {}\nRunning Rule of Five review passes...",
            title
        )));

        // === RULE OF FIVE ITERATION ===
        // Start with Pass 1: Completeness
        let mut current_plan = String::new();
        let mut pass_outputs: Vec<String> = Vec::new();
        let mut consecutive_unchanged = 0;
        const MAX_PASSES: u8 = 5;
        const MIN_PASSES: u8 = 3;

        for pass_num in 1..=MAX_PASSES {
            let focus = match FocusArea::from_pass(pass_num) {
                Some(f) => f,
                None => break,
            };

            info!("=== Rule of Five Pass {}: {} ===", pass_num, focus);

            // Update UI with progress
            self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                "  Pass {}/5: {} review...",
                pass_num,
                focus.name()
            )));

            // Build context for this pass
            let ctx = if pass_num == 1 {
                PromptContext::first_pass(conversation_text.clone())
            } else {
                PromptContext::review_pass(pass_num, current_plan.clone(), focus)
            };

            // Get the pass-specific prompt
            let pass_prompt = match prompt_loader.pass_prompt(pass_num, &ctx) {
                Ok(p) => p,
                Err(e) => {
                    warn!("Failed to load pass {} prompt: {}", pass_num, e);
                    self.app
                        .state_mut()
                        .set_error(format!("Failed to load pass {} prompt", pass_num));
                    return;
                }
            };

            // Build the LLM request
            let user_message = if pass_num == 1 {
                format!(
                    "{}\n\n---\n\n# Conversation to convert to Plan:\n\n{}",
                    pass_prompt, conversation_text
                )
            } else {
                format!("{}\n\n---\n\n# Current Plan:\n\n{}", pass_prompt, current_plan)
            };

            let completion_request = CompletionRequest {
                system_prompt: system_prompt.clone(),
                messages: vec![Message::user(&user_message)],
                tools: vec![],
                max_tokens: 8192,
            };

            // Execute the pass
            info!("Sending pass {} request to LLM...", pass_num);
            let pass_output = match llm.complete(completion_request).await {
                Ok(response) => {
                    let output = response.content.unwrap_or_default();
                    info!("Pass {} complete: {} chars", pass_num, output.len());
                    output
                }
                Err(e) => {
                    warn!("Pass {} failed: {}", pass_num, e);
                    self.app
                        .state_mut()
                        .set_error(format!("Pass {} failed: {}", pass_num, e));
                    return;
                }
            };

            // Check for convergence (no significant changes)
            let changed = self.plan_changed_significantly(&current_plan, &pass_output);

            if changed {
                consecutive_unchanged = 0;
                info!("Pass {} made changes", pass_num);
            } else {
                consecutive_unchanged += 1;
                info!(
                    "Pass {} converged (no changes), consecutive={}",
                    pass_num, consecutive_unchanged
                );
            }

            // Update current plan
            current_plan = pass_output.clone();
            pass_outputs.push(pass_output);

            // Check for early convergence (two consecutive unchanged passes after MIN_PASSES)
            if pass_num >= MIN_PASSES && consecutive_unchanged >= 2 {
                info!("Early convergence after pass {} (two consecutive unchanged)", pass_num);
                self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
                    "  Plan converged after {} passes",
                    pass_num
                )));
                break;
            }
        }

        info!("Rule of Five complete: {} passes executed", pass_outputs.len());

        // Create the plan execution with Draft status
        info!("Creating LoopExecution with Draft status...");
        let mut execution = crate::domain::LoopExecution::new("plan", &title);
        execution.set_title(title.clone());
        execution.set_status(crate::domain::LoopExecutionStatus::Draft);
        info!(
            "LoopExecution created: id={}, loop_type={}, status={:?}",
            execution.id, execution.loop_type, execution.status
        );

        // Store the final plan and pass count in context
        execution.set_context(serde_json::json!({
            "user-request": conversation_text,
            "review-passes-completed": pass_outputs.len(),
            "final-plan": current_plan
        }));

        // Create the execution record
        info!("Calling state_manager.create_execution()...");
        let exec_id = match state_manager.create_execution(execution).await {
            Ok(id) => {
                info!("Draft execution created successfully: {}", id);
                id
            }
            Err(e) => {
                warn!("Failed to create draft execution: {}", e);
                self.app.state_mut().set_error(format!("Failed to create draft: {}", e));
                return;
            }
        };

        // Create the plan directory and file
        let plan_dir = self.worktree.join(".taskdaemon/plans").join(&exec_id);
        info!("Creating plan directory: {:?}", plan_dir);
        if let Err(e) = std::fs::create_dir_all(&plan_dir) {
            warn!("Failed to create plan directory: {}", e);
            self.app
                .state_mut()
                .set_error(format!("Failed to create plan directory: {}", e));
            return;
        }
        info!("Plan directory created successfully");

        // Write the final plan.md file
        let plan_content = format!(
            r#"# Plan: {}

**Status:** Draft
**Created:** {}
**ID:** {}
**Review Passes:** {} of 5

---

{}

---

*This plan has been reviewed through {} Rule of Five passes. Use `s` in the Executions view to start execution.*
"#,
            title,
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            exec_id,
            pass_outputs.len(),
            current_plan,
            pass_outputs.len()
        );

        let plan_path = plan_dir.join("plan.md");
        info!("Writing plan.md to {:?}", plan_path);
        if let Err(e) = std::fs::write(&plan_path, &plan_content) {
            warn!("Failed to write plan file: {}", e);
            self.app
                .state_mut()
                .set_error(format!("Failed to write plan file: {}", e));
            return;
        }
        info!("plan.md written successfully");

        // Extract hash for display (first 6 chars)
        let hash = &exec_id[..6.min(exec_id.len())];

        // Show success message
        self.app.state_mut().repl_history.push(ReplMessage::assistant(format!(
            "Created draft plan: {} ({})\n{} review passes completed.\nView in Executions with Tab, then press `s` to start execution.",
            title, hash, pass_outputs.len()
        )));

        info!(
            "=== create_plan_draft COMPLETE: exec_id={}, title={}, passes={}, plan_path={:?} ===",
            exec_id,
            title,
            pass_outputs.len(),
            plan_path
        );

        // Force refresh to show the new draft
        self.last_refresh = Instant::now() - DATA_REFRESH_INTERVAL;
    }

    /// Check if the plan changed significantly between passes
    ///
    /// Returns true if there are meaningful changes, false if the plans are essentially the same.
    /// This is used to detect convergence in the Rule of Five iteration.
    fn plan_changed_significantly(&self, old_plan: &str, new_plan: &str) -> bool {
        if old_plan.is_empty() {
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
    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        use crossterm::event::MouseEventKind;

        let state = self.app.state_mut();

        // Only handle scroll in REPL view
        if !matches!(state.current_view, View::Repl) {
            return;
        }

        let max = state.repl_max_scroll;
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                state.repl_scroll_up(3, max);
            }
            MouseEventKind::ScrollDown => {
                state.repl_scroll_down(3, max);
            }
            _ => {
                // Other mouse events (click, drag) not handled yet
            }
        }
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
                state.executions_draft = items.iter().filter(|r| r.status == "draft").count();
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

                    // Extract plan content from context if available
                    let plan_content = exec
                        .context
                        .get("final-plan")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

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
