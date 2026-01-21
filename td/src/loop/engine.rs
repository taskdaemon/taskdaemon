//! LoopEngine - executes Ralph Wiggum loop iterations

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use handlebars::Handlebars;
use tracing::{debug, info, warn};

use crate::coordinator::{CoordMessage, CoordinatorHandle};
use crate::domain::{IterationLog, Priority, ToolCallSummary};
use crate::llm::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmClient, Message, StopReason, TokenUsage, ToolDefinition,
};
use crate::progress::{IterationContext, ProgressStrategy, SystemCapturedProgress};
use crate::scheduler::Scheduler;
use crate::state::StateManager;
use crate::tools::{ToolContext, ToolExecutor, ToolResult};

use super::LoopConfig;
use super::validation::run_validation;

/// Status of a loop execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopStatus {
    Running,
    Paused,
    Rebasing,
    Blocked { reason: String },
    Complete,
    Failed { reason: String },
    Stopped,
}

/// Result of a single iteration
#[derive(Debug)]
pub enum IterationResult {
    /// Loop completed successfully
    Complete { iterations: u32 },
    /// Continue to next iteration
    Continue { validation_output: String, exit_code: i32 },
    /// Rate limited, should retry after delay
    RateLimited { retry_after: Duration },
    /// Loop was interrupted (stop signal, etc)
    Interrupted { reason: String },
    /// Error occurred
    Error { message: String, recoverable: bool },
}

/// Loop execution engine
pub struct LoopEngine {
    /// Execution ID
    pub exec_id: String,

    /// Loop configuration
    config: LoopConfig,

    /// LLM client
    llm: Arc<dyn LlmClient>,

    /// Tool executor
    tool_executor: ToolExecutor,

    /// Progress tracker
    progress: Box<dyn ProgressStrategy>,

    /// Worktree path
    worktree: PathBuf,

    /// Current iteration number
    iteration: u32,

    /// Current status
    status: LoopStatus,

    /// Template engine
    #[allow(dead_code)]
    handlebars: Handlebars<'static>,

    /// Coordinator handle for inter-loop communication
    coord_handle: Option<CoordinatorHandle>,

    /// Scheduler for rate limiting LLM calls
    scheduler: Option<Arc<Scheduler>>,

    /// Execution context from LoopExecution (for parent content, etc.)
    execution_context: serde_json::Value,

    /// Main repo root (for reading parent files)
    repo_root: PathBuf,

    /// State manager for persisting iteration logs
    state: Option<StateManager>,

    /// Tool call buffer for the current iteration
    tool_call_buffer: Vec<ToolCallSummary>,

    /// Token usage accumulated in the current iteration
    iteration_token_usage: TokenUsage,
}

impl LoopEngine {
    /// Create a new loop engine
    pub fn new(exec_id: String, config: LoopConfig, llm: Arc<dyn LlmClient>, worktree: PathBuf) -> Self {
        debug!(%exec_id, loop_type = %config.loop_type, ?worktree, "LoopEngine::new: called");
        let progress = Box::new(SystemCapturedProgress::with_limits(
            config.progress_max_entries,
            config.progress_max_chars,
        ));

        Self {
            exec_id,
            config,
            llm,
            tool_executor: ToolExecutor::standard(),
            progress,
            worktree: worktree.clone(),
            iteration: 0,
            status: LoopStatus::Running,
            handlebars: Handlebars::new(),
            coord_handle: None,
            scheduler: None,
            execution_context: serde_json::json!({}),
            repo_root: worktree,
            state: None,
            tool_call_buffer: Vec::new(),
            iteration_token_usage: TokenUsage::default(),
        }
    }

    /// Create a new loop engine with coordinator handle for inter-loop communication
    pub fn with_coordinator(
        exec_id: String,
        config: LoopConfig,
        llm: Arc<dyn LlmClient>,
        worktree: PathBuf,
        coord_handle: CoordinatorHandle,
    ) -> Self {
        debug!(%exec_id, loop_type = %config.loop_type, ?worktree, "LoopEngine::with_coordinator: called");
        let progress = Box::new(SystemCapturedProgress::with_limits(
            config.progress_max_entries,
            config.progress_max_chars,
        ));

        Self {
            exec_id,
            config,
            llm,
            tool_executor: ToolExecutor::standard(),
            progress,
            worktree: worktree.clone(),
            iteration: 0,
            status: LoopStatus::Running,
            handlebars: Handlebars::new(),
            coord_handle: Some(coord_handle),
            scheduler: None,
            execution_context: serde_json::json!({}),
            repo_root: worktree,
            state: None,
            tool_call_buffer: Vec::new(),
            iteration_token_usage: TokenUsage::default(),
        }
    }

    /// Set the execution context (from LoopExecution.context)
    pub fn with_execution_context(mut self, context: serde_json::Value) -> Self {
        debug!(exec_id = %self.exec_id, "with_execution_context: called");
        self.execution_context = context;
        self
    }

    /// Set the repo root path (for reading parent files)
    pub fn with_repo_root(mut self, repo_root: PathBuf) -> Self {
        debug!(exec_id = %self.exec_id, ?repo_root, "with_repo_root: called");
        self.repo_root = repo_root;
        self
    }

    /// Set the scheduler for rate limiting LLM calls
    pub fn with_scheduler(mut self, scheduler: Arc<Scheduler>) -> Self {
        debug!(exec_id = %self.exec_id, "with_scheduler: called");
        self.scheduler = Some(scheduler);
        self
    }

    /// Set the state manager for persisting iteration logs
    pub fn with_state(mut self, state: StateManager) -> Self {
        debug!(exec_id = %self.exec_id, "with_state: called");
        self.state = Some(state);
        self
    }

    /// Get the accumulated progress text
    ///
    /// This returns the progress text that should be persisted to LoopExecution
    /// for crash recovery.
    pub fn get_progress(&self) -> String {
        debug!(exec_id = %self.exec_id, "get_progress: called");
        self.progress.get_progress()
    }

    /// Get the current iteration number
    pub fn current_iteration(&self) -> u32 {
        debug!(exec_id = %self.exec_id, iteration = self.iteration, "current_iteration: called");
        self.iteration
    }

    /// Run the loop until completion or max iterations
    pub async fn run(&mut self) -> eyre::Result<IterationResult> {
        debug!(exec_id = %self.exec_id, loop_type = %self.config.loop_type, max_iterations = self.config.max_iterations, "run: called");
        info!(
            "Starting loop {} (type: {}, max_iterations: {})",
            self.exec_id, self.config.loop_type, self.config.max_iterations
        );

        // Subscribe to main_updated alerts if coordinator is available
        if let Some(ref coord_handle) = self.coord_handle {
            debug!(exec_id = %self.exec_id, "run: subscribing to main_updated");
            if let Err(e) = coord_handle.subscribe("main_updated").await {
                warn!("Failed to subscribe to main_updated: {}", e);
            }
        } else {
            debug!(exec_id = %self.exec_id, "run: no coordinator handle, skipping subscription");
        }

        while self.iteration < self.config.max_iterations {
            debug!(exec_id = %self.exec_id, iteration = self.iteration, max = self.config.max_iterations, "run: loop iteration start");
            // Check for coordinator messages before each iteration
            if let Some(result) = self.poll_coordinator_messages().await {
                debug!(exec_id = %self.exec_id, "run: coordinator message caused early return");
                return Ok(result);
            }

            self.iteration += 1;
            info!(
                "Loop {} iteration {}/{}",
                self.exec_id, self.iteration, self.config.max_iterations
            );

            let result = self.run_iteration().await?;

            match result {
                IterationResult::Complete { .. } => {
                    debug!(exec_id = %self.exec_id, "run: iteration complete, loop finished");
                    self.status = LoopStatus::Complete;
                    return Ok(result);
                }
                IterationResult::Continue { .. } => {
                    debug!(exec_id = %self.exec_id, "run: iteration continue, sleeping before next");
                    // Continue to next iteration
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                IterationResult::RateLimited { retry_after } => {
                    debug!(exec_id = %self.exec_id, ?retry_after, "run: rate limited");
                    warn!("Rate limited, sleeping for {:?}", retry_after);
                    tokio::time::sleep(retry_after).await;
                    self.iteration -= 1; // Don't count this iteration
                }
                IterationResult::Interrupted { reason } => {
                    debug!(exec_id = %self.exec_id, %reason, "run: interrupted");
                    self.status = LoopStatus::Stopped;
                    return Ok(IterationResult::Interrupted { reason });
                }
                IterationResult::Error { message, recoverable } => {
                    if !recoverable {
                        debug!(exec_id = %self.exec_id, %message, "run: non-recoverable error");
                        self.status = LoopStatus::Failed {
                            reason: message.clone(),
                        };
                        return Ok(IterationResult::Error { message, recoverable });
                    }
                    debug!(exec_id = %self.exec_id, %message, "run: recoverable error, continuing");
                    warn!("Recoverable error: {}", message);
                }
            }
        }

        debug!(exec_id = %self.exec_id, max_iterations = self.config.max_iterations, "run: max iterations exceeded");
        self.status = LoopStatus::Failed {
            reason: "Max iterations exceeded".to_string(),
        };
        Ok(IterationResult::Error {
            message: format!("Max iterations ({}) exceeded", self.config.max_iterations),
            recoverable: false,
        })
    }

    /// Poll for coordinator messages (non-blocking)
    ///
    /// Returns Some(IterationResult) if the loop should stop, None to continue.
    async fn poll_coordinator_messages(&mut self) -> Option<IterationResult> {
        debug!(exec_id = %self.exec_id, "poll_coordinator_messages: called");
        // Collect all pending messages first to avoid borrow conflicts
        let messages: Vec<CoordMessage> = {
            let coord_handle = self.coord_handle.as_ref()?;
            let mut msgs = Vec::new();
            while let Some(msg) = coord_handle.try_recv() {
                msgs.push(msg);
            }
            msgs
        };
        debug!(exec_id = %self.exec_id, message_count = messages.len(), "poll_coordinator_messages: collected messages");

        // Process collected messages
        for msg in messages {
            match msg {
                CoordMessage::Stop { from_exec_id, reason } => {
                    debug!(exec_id = %self.exec_id, %from_exec_id, %reason, "poll_coordinator_messages: received stop");
                    info!(
                        "Loop {} received stop request from {}: {}",
                        self.exec_id, from_exec_id, reason
                    );
                    self.status = LoopStatus::Stopped;
                    return Some(IterationResult::Interrupted {
                        reason: format!("Stop requested by {}: {}", from_exec_id, reason),
                    });
                }
                CoordMessage::Query {
                    query_id,
                    from_exec_id,
                    question,
                } => {
                    debug!(exec_id = %self.exec_id, %query_id, %from_exec_id, %question, "poll_coordinator_messages: received query");
                    // Handle query - for now, respond with a default message
                    // In future, this could be expanded to support custom query handlers
                    info!(
                        "Loop {} received query {} from {}: {}",
                        self.exec_id, query_id, from_exec_id, question
                    );
                    if let Some(ref coord_handle) = self.coord_handle
                        && let Err(e) = coord_handle
                            .reply_query(&query_id, &format!("Loop {} cannot answer queries yet", self.exec_id))
                            .await
                    {
                        warn!("Failed to reply to query: {}", e);
                    }
                }
                CoordMessage::Share {
                    from_exec_id,
                    share_type,
                    data,
                } => {
                    debug!(exec_id = %self.exec_id, %from_exec_id, %share_type, "poll_coordinator_messages: received share");
                    // Log shared data - could be used for inter-loop coordination
                    info!(
                        "Loop {} received share '{}' from {}: {}",
                        self.exec_id, share_type, from_exec_id, data
                    );
                }
                CoordMessage::Notification {
                    from_exec_id,
                    event_type,
                    data,
                } => {
                    debug!(exec_id = %self.exec_id, %from_exec_id, %event_type, "poll_coordinator_messages: received notification");
                    // Handle notifications - main_updated is particularly important
                    if event_type == "main_updated" {
                        debug!(exec_id = %self.exec_id, "poll_coordinator_messages: main_updated notification");
                        info!(
                            "Loop {} received main_updated notification from {}: {}",
                            self.exec_id, from_exec_id, data
                        );

                        // Extract new SHA from notification data
                        let new_sha = data.get("new_sha").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let branch = data
                            .get("branch")
                            .and_then(|v| v.as_str())
                            .unwrap_or("main")
                            .to_string();

                        // Perform rebase
                        debug!(exec_id = %self.exec_id, %branch, ?new_sha, "poll_coordinator_messages: performing rebase");
                        match self.handle_rebase(&branch, new_sha.as_deref()).await {
                            Ok(()) => {
                                debug!(exec_id = %self.exec_id, "poll_coordinator_messages: rebase successful");
                                info!("Loop {} rebase successful, resuming", self.exec_id);
                                self.status = LoopStatus::Running;
                            }
                            Err(e) => {
                                debug!(exec_id = %self.exec_id, error = %e, "poll_coordinator_messages: rebase failed");
                                warn!("Loop {} rebase failed: {}", self.exec_id, e);
                                self.status = LoopStatus::Blocked {
                                    reason: format!("Rebase conflict: {}", e),
                                };
                                // Return interruption so the loop can handle the blocked state
                                return Some(IterationResult::Interrupted {
                                    reason: format!("Rebase failed: {}", e),
                                });
                            }
                        }
                    } else {
                        debug!(exec_id = %self.exec_id, %event_type, "poll_coordinator_messages: non-main_updated notification");
                        info!(
                            "Loop {} received notification '{}' from {}: {}",
                            self.exec_id, event_type, from_exec_id, data
                        );
                    }
                }
            }
        }

        debug!(exec_id = %self.exec_id, "poll_coordinator_messages: no stop required");
        None
    }

    /// Handle rebase when main branch is updated
    ///
    /// This pauses the loop, performs a git rebase onto the updated main branch,
    /// and resumes execution. If rebase conflicts occur, the loop enters Blocked state.
    async fn handle_rebase(&mut self, branch: &str, _new_sha: Option<&str>) -> eyre::Result<()> {
        debug!(exec_id = %self.exec_id, %branch, "handle_rebase: called");
        self.status = LoopStatus::Rebasing;

        info!("Loop {} rebasing onto {}", self.exec_id, branch);

        // Fetch latest from remote (in case we haven't already)
        debug!(exec_id = %self.exec_id, %branch, "handle_rebase: fetching from origin");
        let fetch_output = tokio::process::Command::new("git")
            .args(["fetch", "origin", branch])
            .current_dir(&self.worktree)
            .output()
            .await?;

        if !fetch_output.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_output.stderr);
            debug!(exec_id = %self.exec_id, %stderr, "handle_rebase: fetch warning");
            warn!("Git fetch warning: {}", stderr);
            // Don't fail on fetch errors - we may be able to rebase locally
        } else {
            debug!(exec_id = %self.exec_id, "handle_rebase: fetch successful");
        }

        // Attempt rebase onto origin/branch
        let remote_branch = format!("origin/{}", branch);
        debug!(exec_id = %self.exec_id, %remote_branch, "handle_rebase: attempting rebase");
        let rebase_output = tokio::process::Command::new("git")
            .args(["rebase", &remote_branch])
            .current_dir(&self.worktree)
            .output()
            .await?;

        if !rebase_output.status.success() {
            let stderr = String::from_utf8_lossy(&rebase_output.stderr);
            debug!(exec_id = %self.exec_id, %stderr, "handle_rebase: rebase failed, aborting");

            // Abort the failed rebase to return to a clean state
            let _ = tokio::process::Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(&self.worktree)
                .output()
                .await;

            return Err(eyre::eyre!("Rebase failed: {}", stderr));
        }

        debug!(exec_id = %self.exec_id, "handle_rebase: rebase complete");
        info!("Loop {} rebase complete", self.exec_id);
        Ok(())
    }

    /// Run a single iteration
    async fn run_iteration(&mut self) -> eyre::Result<IterationResult> {
        debug!(exec_id = %self.exec_id, iteration = self.iteration, "run_iteration: called");

        // Clear iteration-level tracking
        self.tool_call_buffer.clear();
        self.iteration_token_usage = TokenUsage::default();

        // Build context for template
        let context = self.build_template_context().await?;
        debug!(exec_id = %self.exec_id, "run_iteration: built template context");

        // Render prompt
        let prompt = self.render_prompt(&context)?;
        debug!(exec_id = %self.exec_id, prompt_len = prompt.len(), "run_iteration: rendered prompt");

        // Create tool context for this iteration - with coordinator if available
        let tool_ctx = if let Some(ref coord_handle) = self.coord_handle {
            debug!(exec_id = %self.exec_id, "run_iteration: creating tool context with coordinator");
            ToolContext::with_coordinator(self.worktree.clone(), self.exec_id.clone(), coord_handle.clone())
        } else {
            debug!(exec_id = %self.exec_id, "run_iteration: creating tool context without coordinator");
            ToolContext::new(self.worktree.clone(), self.exec_id.clone())
        };
        tool_ctx.clear_reads().await;

        // Get tool definitions for this loop type
        let tool_defs = self.tool_executor.definitions_for(&self.config.tools);
        debug!(exec_id = %self.exec_id, tool_count = tool_defs.len(), "run_iteration: got tool definitions");

        // Run agentic loop (LLM + tool calls until EndTurn)
        debug!(exec_id = %self.exec_id, "run_iteration: starting agentic loop");
        let result = self.run_agentic_loop(&prompt, &tool_ctx, &tool_defs).await?;

        match result {
            AgenticLoopResult::Complete => {
                debug!(exec_id = %self.exec_id, "run_iteration: agentic loop complete");
            }
            AgenticLoopResult::RateLimited { retry_after } => {
                debug!(exec_id = %self.exec_id, ?retry_after, "run_iteration: agentic loop rate limited");
                return Ok(IterationResult::RateLimited { retry_after });
            }
            AgenticLoopResult::Error { message, recoverable } => {
                debug!(exec_id = %self.exec_id, %message, recoverable, "run_iteration: agentic loop error");
                return Ok(IterationResult::Error { message, recoverable });
            }
        }

        // Run validation
        debug!(exec_id = %self.exec_id, command = %self.config.validation_command, "run_iteration: running validation");
        let validation = run_validation(
            &self.config.validation_command,
            &self.worktree,
            Duration::from_millis(self.config.iteration_timeout_ms),
        )
        .await?;
        debug!(exec_id = %self.exec_id, exit_code = validation.exit_code, duration_ms = validation.duration_ms, "run_iteration: validation complete");

        // Record progress
        let files_changed = self.get_changed_files().await;
        debug!(exec_id = %self.exec_id, files_changed_count = files_changed.len(), "run_iteration: got changed files");
        let iter_ctx = IterationContext::new(
            self.iteration,
            &self.config.validation_command,
            validation.exit_code,
            &validation.stdout,
            &validation.stderr,
            validation.duration_ms,
            files_changed.clone(),
        );
        self.progress.record(&iter_ctx);

        // Persist iteration log with FULL validation output (before truncation)
        if let Some(ref state) = self.state {
            let log = IterationLog::new(&self.exec_id, self.iteration)
                .with_validation_command(&self.config.validation_command)
                .with_exit_code(validation.exit_code)
                .with_stdout(&validation.stdout)
                .with_stderr(&validation.stderr)
                .with_duration_ms(validation.duration_ms)
                .with_files_changed(files_changed)
                .with_llm_tokens(
                    Some(self.iteration_token_usage.input_tokens),
                    Some(self.iteration_token_usage.output_tokens),
                )
                .with_tool_calls(std::mem::take(&mut self.tool_call_buffer));

            if let Err(e) = state.create_iteration_log(log).await {
                warn!(exec_id = %self.exec_id, iteration = self.iteration, error = %e, "Failed to persist iteration log");
            } else {
                debug!(exec_id = %self.exec_id, iteration = self.iteration, "run_iteration: persisted iteration log");
            }

            // Update aggregate metrics on the LoopExecution
            if let Ok(Some(mut exec)) = state.get_execution(&self.exec_id).await {
                exec.add_iteration_metrics(
                    self.iteration_token_usage.input_tokens,
                    self.iteration_token_usage.output_tokens,
                    validation.duration_ms,
                );
                if let Err(e) = state.update_execution(exec).await {
                    warn!(exec_id = %self.exec_id, error = %e, "Failed to update execution metrics");
                }
            }
        }

        // Check if validation passed
        if validation.passed(self.config.success_exit_code) {
            debug!(exec_id = %self.exec_id, "run_iteration: validation passed");
            info!(
                "Loop {} completed successfully after {} iterations",
                self.exec_id, self.iteration
            );
            return Ok(IterationResult::Complete {
                iterations: self.iteration,
            });
        }

        debug!(exec_id = %self.exec_id, exit_code = validation.exit_code, "run_iteration: validation failed");
        info!(
            "Loop {} iteration {} validation failed (exit code: {})",
            self.exec_id, self.iteration, validation.exit_code
        );

        Ok(IterationResult::Continue {
            validation_output: if !validation.stdout.is_empty() {
                validation.stdout
            } else {
                validation.stderr
            },
            exit_code: validation.exit_code,
        })
    }

    /// Run the agentic tool loop within an iteration
    async fn run_agentic_loop(
        &mut self,
        initial_prompt: &str,
        tool_ctx: &ToolContext,
        tool_defs: &[ToolDefinition],
    ) -> eyre::Result<AgenticLoopResult> {
        debug!(exec_id = %self.exec_id, prompt_len = initial_prompt.len(), tool_count = tool_defs.len(), "run_agentic_loop: called");
        let system_prompt = format!(
            "You are an AI assistant working on a task. Complete the task using the available tools.\n\
             Working directory: {}\n\
             Loop type: {}",
            self.worktree.display(),
            self.config.loop_type
        );

        let mut messages = vec![Message::user(initial_prompt)];
        let mut turn = 0;

        loop {
            turn += 1;
            debug!(exec_id = %self.exec_id, turn, max_turns = self.config.max_turns_per_iteration, "run_agentic_loop: turn start");

            if turn > self.config.max_turns_per_iteration as usize {
                debug!(exec_id = %self.exec_id, "run_agentic_loop: max turns reached");
                warn!(
                    "Max turns ({}) reached in iteration",
                    self.config.max_turns_per_iteration
                );
                break;
            }

            let request = CompletionRequest {
                system_prompt: system_prompt.clone(),
                messages: messages.clone(),
                tools: tool_defs.to_vec(),
                max_tokens: self.config.max_tokens,
            };

            // Wait for scheduler slot (rate limiting) before making LLM call
            if let Some(scheduler) = &self.scheduler {
                // Use a turn-specific ID for per-turn rate limiting
                let turn_id = format!("{}-turn-{}", self.exec_id, turn);
                debug!(exec_id = %self.exec_id, %turn_id, "run_agentic_loop: waiting for scheduler slot");
                if let Err(e) = scheduler.wait_for_slot(&turn_id, Priority::Normal).await {
                    debug!(exec_id = %self.exec_id, error = %e, "run_agentic_loop: scheduler error");
                    return Ok(AgenticLoopResult::Error {
                        message: format!("Scheduler error: {}", e),
                        recoverable: true,
                    });
                }
                debug!(exec_id = %self.exec_id, "run_agentic_loop: got scheduler slot");
            } else {
                debug!(exec_id = %self.exec_id, "run_agentic_loop: no scheduler, skipping rate limit");
            }

            debug!(exec_id = %self.exec_id, turn, "run_agentic_loop: calling LLM");
            let response = match self.llm.complete(request).await {
                Ok(r) => {
                    debug!(exec_id = %self.exec_id, turn, stop_reason = ?r.stop_reason, "run_agentic_loop: LLM response received");
                    // Mark scheduler slot as complete after successful call
                    if let Some(scheduler) = &self.scheduler {
                        let turn_id = format!("{}-turn-{}", self.exec_id, turn);
                        scheduler.complete(&turn_id).await;
                    }
                    // Accumulate token usage for this iteration
                    self.iteration_token_usage.input_tokens += r.usage.input_tokens;
                    self.iteration_token_usage.output_tokens += r.usage.output_tokens;
                    r
                }
                Err(e) if e.is_rate_limit() => {
                    debug!(exec_id = %self.exec_id, turn, "run_agentic_loop: LLM rate limited");
                    // Mark slot complete even on rate limit
                    if let Some(scheduler) = &self.scheduler {
                        let turn_id = format!("{}-turn-{}", self.exec_id, turn);
                        scheduler.complete(&turn_id).await;
                    }
                    return Ok(AgenticLoopResult::RateLimited {
                        retry_after: e.retry_after().unwrap_or(Duration::from_secs(60)),
                    });
                }
                Err(e) if e.is_retryable() => {
                    debug!(exec_id = %self.exec_id, turn, error = %e, "run_agentic_loop: LLM retryable error");
                    // Mark slot complete on retryable error
                    if let Some(scheduler) = &self.scheduler {
                        let turn_id = format!("{}-turn-{}", self.exec_id, turn);
                        scheduler.complete(&turn_id).await;
                    }
                    return Ok(AgenticLoopResult::Error {
                        message: e.to_string(),
                        recoverable: true,
                    });
                }
                Err(e) => {
                    debug!(exec_id = %self.exec_id, turn, error = %e, "run_agentic_loop: LLM non-retryable error");
                    // Mark slot complete on non-retryable error
                    if let Some(scheduler) = &self.scheduler {
                        let turn_id = format!("{}-turn-{}", self.exec_id, turn);
                        scheduler.complete(&turn_id).await;
                    }
                    return Ok(AgenticLoopResult::Error {
                        message: e.to_string(),
                        recoverable: false,
                    });
                }
            };

            // Build assistant message from response
            let assistant_message = self.build_assistant_message(&response);
            messages.push(assistant_message);

            match response.stop_reason {
                StopReason::EndTurn => {
                    debug!(exec_id = %self.exec_id, turn, "run_agentic_loop: LLM ended turn");
                    // LLM finished its turn
                    break;
                }
                StopReason::ToolUse => {
                    debug!(exec_id = %self.exec_id, turn, tool_count = response.tool_calls.len(), "run_agentic_loop: LLM requested tool use");
                    // Execute tools and continue
                    let tool_results = self.execute_tools(&response.tool_calls, tool_ctx).await;
                    debug!(exec_id = %self.exec_id, turn, results_count = tool_results.len(), "run_agentic_loop: tools executed");

                    // Record tool call summaries for iteration log
                    for (call, (_, result)) in response.tool_calls.iter().zip(tool_results.iter()) {
                        let args_summary = call.input.to_string();
                        let summary = ToolCallSummary::new(&call.name, &args_summary, &result.content, result.is_error);
                        self.tool_call_buffer.push(summary);
                    }

                    // Build user message with tool results
                    let tool_result_message = self.build_tool_result_message(&tool_results);
                    messages.push(tool_result_message);
                }
                StopReason::MaxTokens => {
                    debug!(exec_id = %self.exec_id, turn, "run_agentic_loop: LLM hit max tokens");
                    // Output truncated, ask to continue
                    messages.push(Message::user(
                        "Continue from where you left off. Your previous response was truncated.",
                    ));
                }
                StopReason::StopSequence => {
                    debug!(exec_id = %self.exec_id, turn, "run_agentic_loop: LLM hit stop sequence");
                    break;
                }
            }
        }

        debug!(exec_id = %self.exec_id, "run_agentic_loop: complete");
        Ok(AgenticLoopResult::Complete)
    }

    /// Execute tool calls and return results
    async fn execute_tools(&self, tool_calls: &[crate::llm::ToolCall], ctx: &ToolContext) -> Vec<(String, ToolResult)> {
        debug!(exec_id = %self.exec_id, tool_count = tool_calls.len(), "execute_tools: called");
        self.tool_executor.execute_all(tool_calls, ctx).await
    }

    /// Build assistant message from response
    fn build_assistant_message(&self, response: &CompletionResponse) -> Message {
        debug!(exec_id = %self.exec_id, has_content = response.content.is_some(), tool_calls = response.tool_calls.len(), "build_assistant_message: called");
        let mut blocks = Vec::new();

        if let Some(text) = &response.content {
            debug!(exec_id = %self.exec_id, "build_assistant_message: adding text block");
            blocks.push(ContentBlock::text(text));
        }

        for call in &response.tool_calls {
            debug!(exec_id = %self.exec_id, tool_name = %call.name, "build_assistant_message: adding tool use block");
            blocks.push(ContentBlock::ToolUse {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.input.clone(),
            });
        }

        Message::assistant_blocks(blocks)
    }

    /// Build user message with tool results
    fn build_tool_result_message(&self, results: &[(String, ToolResult)]) -> Message {
        debug!(exec_id = %self.exec_id, result_count = results.len(), "build_tool_result_message: called");
        let blocks: Vec<ContentBlock> = results
            .iter()
            .map(|(id, result)| {
                debug!(exec_id = %self.exec_id, %id, is_error = result.is_error, "build_tool_result_message: adding result");
                ContentBlock::tool_result(id, &result.content, result.is_error)
            })
            .collect();

        Message::user_blocks(blocks)
    }

    /// Build template context for prompt rendering
    async fn build_template_context(&self) -> eyre::Result<HashMap<String, String>> {
        debug!(exec_id = %self.exec_id, "build_template_context: called");
        let mut context = HashMap::new();

        // Basic loop info
        context.insert("working-directory".to_string(), self.worktree.display().to_string());
        context.insert("iteration".to_string(), self.iteration.to_string());
        debug!(exec_id = %self.exec_id, "build_template_context: added basic info");

        // Add execution context values (from cascade)
        self.populate_execution_context(&mut context);

        // Read parent content from file if this is a child loop
        self.populate_parent_content(&mut context).await;

        // Git status
        debug!(exec_id = %self.exec_id, "build_template_context: getting git status");
        if let Ok(output) = tokio::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.worktree)
            .output()
            .await
        {
            let status = String::from_utf8_lossy(&output.stdout).to_string();
            debug!(exec_id = %self.exec_id, status_len = status.len(), "build_template_context: got git status");
            context.insert("git-status".to_string(), status);
        } else {
            debug!(exec_id = %self.exec_id, "build_template_context: failed to get git status");
        }

        // Git diff
        debug!(exec_id = %self.exec_id, "build_template_context: getting git diff");
        if let Ok(output) = tokio::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&self.worktree)
            .output()
            .await
        {
            let diff = String::from_utf8_lossy(&output.stdout).to_string();
            // Truncate large diffs
            let truncated = if diff.len() > 5000 {
                debug!(exec_id = %self.exec_id, original_len = diff.len(), "build_template_context: truncating large diff");
                format!("{}...\n[diff truncated]", &diff[..5000])
            } else {
                debug!(exec_id = %self.exec_id, diff_len = diff.len(), "build_template_context: diff fits");
                diff
            };
            context.insert("git-diff".to_string(), truncated);
        } else {
            debug!(exec_id = %self.exec_id, "build_template_context: failed to get git diff");
        }

        // Progress from previous iterations
        debug!(exec_id = %self.exec_id, "build_template_context: adding progress");
        context.insert("progress".to_string(), self.progress.get_progress());

        debug!(exec_id = %self.exec_id, context_keys = context.len(), "build_template_context: complete");
        Ok(context)
    }

    /// Populate template context from execution context (cascade values)
    fn populate_execution_context(&self, context: &mut HashMap<String, String>) {
        debug!(exec_id = %self.exec_id, "populate_execution_context: called");

        // Copy string values from execution_context to template context
        if let Some(obj) = self.execution_context.as_object() {
            for (key, value) in obj {
                if let Some(s) = value.as_str() {
                    debug!(exec_id = %self.exec_id, %key, "populate_execution_context: adding value");
                    context.insert(key.clone(), s.to_string());
                }
            }
        }
    }

    /// Populate parent content from file (for cascade child loops)
    async fn populate_parent_content(&self, context: &mut HashMap<String, String>) {
        debug!(exec_id = %self.exec_id, "populate_parent_content: called");

        // Get parent type and file path from execution context
        let parent_type = self.execution_context.get("parent-type").and_then(|v| v.as_str());
        let parent_file = self.execution_context.get("parent-file").and_then(|v| v.as_str());

        debug!(exec_id = %self.exec_id, ?parent_type, ?parent_file, "populate_parent_content: checking parent");

        // If we have a parent file, read it and set the appropriate content variable
        if let Some(file_path) = parent_file {
            // Try both repo_root and worktree paths
            let full_path = if file_path.starts_with('/') {
                PathBuf::from(file_path)
            } else {
                self.repo_root.join(file_path)
            };

            debug!(exec_id = %self.exec_id, ?full_path, "populate_parent_content: reading parent file");

            match tokio::fs::read_to_string(&full_path).await {
                Ok(content) => {
                    info!(exec_id = %self.exec_id, file = ?full_path, len = content.len(), "Read parent content");

                    // Map parent type to the correct template variable
                    let var_name = match parent_type {
                        Some("plan") => "plan-content",
                        Some("spec") => "spec-content",
                        Some("phase") => "phase-content",
                        _ => "parent-content",
                    };

                    debug!(exec_id = %self.exec_id, %var_name, "populate_parent_content: setting variable");
                    context.insert(var_name.to_string(), content);
                }
                Err(e) => {
                    warn!(exec_id = %self.exec_id, file = ?full_path, error = %e, "Failed to read parent file");
                }
            }
        }

        // Also try to find and read output file from context (for user-request, etc.)
        if let Some(output_file) = self.execution_context.get("output-file").and_then(|v| v.as_str()) {
            let output_path = self.worktree.join(output_file);
            if let Ok(content) = tokio::fs::read_to_string(&output_path).await {
                debug!(exec_id = %self.exec_id, file = ?output_path, "populate_parent_content: read output file");
                context.insert("current-plan".to_string(), content);
            }
        }
    }

    /// Render the prompt template with context
    fn render_prompt(&self, context: &HashMap<String, String>) -> eyre::Result<String> {
        debug!(exec_id = %self.exec_id, context_keys = context.len(), "render_prompt: called");
        // For now, use simple string replacement since Handlebars setup is complex
        let mut result = self.config.prompt_template.clone();

        for (key, value) in context {
            let placeholder = format!("{{{{{}}}}}", key);
            result = result.replace(&placeholder, value);
        }

        debug!(exec_id = %self.exec_id, result_len = result.len(), "render_prompt: complete");
        Ok(result)
    }

    /// Get list of changed files from git status
    async fn get_changed_files(&self) -> Vec<String> {
        debug!(exec_id = %self.exec_id, "get_changed_files: called");
        let output = tokio::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.worktree)
            .output()
            .await;

        match output {
            Ok(out) => {
                let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|line| {
                        let trimmed = line.trim();
                        if trimmed.len() > 3 { Some(trimmed[3..].to_string()) } else { None }
                    })
                    .collect();
                debug!(exec_id = %self.exec_id, file_count = files.len(), "get_changed_files: found files");
                files
            }
            Err(e) => {
                debug!(exec_id = %self.exec_id, error = %e, "get_changed_files: failed to get status");
                vec![]
            }
        }
    }

    /// Get current iteration count
    pub fn iteration(&self) -> u32 {
        debug!(exec_id = %self.exec_id, iteration = self.iteration, "iteration: called");
        self.iteration
    }

    /// Get current status
    pub fn status(&self) -> &LoopStatus {
        debug!(exec_id = %self.exec_id, status = ?self.status, "status: called");
        &self.status
    }
}

/// Result of the agentic loop within an iteration
enum AgenticLoopResult {
    Complete,
    RateLimited { retry_after: Duration },
    Error { message: String, recoverable: bool },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::TokenUsage;
    use crate::llm::client::mock::MockLlmClient;
    use tempfile::tempdir;

    #[allow(dead_code)]
    fn make_mock_response(content: &str) -> CompletionResponse {
        CompletionResponse {
            content: Some(content.to_string()),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        }
    }

    #[tokio::test]
    async fn test_loop_engine_creation() {
        let temp = tempdir().unwrap();
        let config = LoopConfig::default();
        let llm = Arc::new(MockLlmClient::new(vec![]));

        let engine = LoopEngine::new("test-exec".to_string(), config, llm, temp.path().to_path_buf());

        assert_eq!(engine.iteration(), 0);
        assert_eq!(*engine.status(), LoopStatus::Running);
    }

    #[tokio::test]
    async fn test_build_template_context() {
        let temp = tempdir().unwrap();
        // Initialize git repo
        tokio::process::Command::new("git")
            .args(["init"])
            .current_dir(temp.path())
            .output()
            .await
            .unwrap();

        let config = LoopConfig::default();
        let llm = Arc::new(MockLlmClient::new(vec![]));
        let engine = LoopEngine::new("test-exec".to_string(), config, llm, temp.path().to_path_buf());

        let context = engine.build_template_context().await.unwrap();

        assert!(context.contains_key("working-directory"));
        assert!(context.contains_key("git-status"));
        assert!(context.contains_key("progress"));
    }

    #[tokio::test]
    async fn test_render_prompt() {
        let temp = tempdir().unwrap();
        let config = LoopConfig {
            prompt_template: "Working in {{working-directory}}, iteration {{iteration}}".to_string(),
            ..Default::default()
        };

        let llm = Arc::new(MockLlmClient::new(vec![]));
        let engine = LoopEngine::new("test-exec".to_string(), config, llm, temp.path().to_path_buf());

        let mut context = HashMap::new();
        context.insert("working-directory".to_string(), "/tmp/test".to_string());
        context.insert("iteration".to_string(), "5".to_string());

        let result = engine.render_prompt(&context).unwrap();

        assert!(result.contains("/tmp/test"));
        assert!(result.contains("5"));
    }
}
