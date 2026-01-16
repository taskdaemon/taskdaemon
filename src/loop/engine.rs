//! LoopEngine - executes Ralph Wiggum loop iterations

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use handlebars::Handlebars;
use tracing::{info, warn};

use crate::llm::{CompletionRequest, CompletionResponse, ContentBlock, LlmClient, Message, StopReason, ToolDefinition};
use crate::progress::{IterationContext, ProgressStrategy, SystemCapturedProgress};
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
}

impl LoopEngine {
    /// Create a new loop engine
    pub fn new(exec_id: String, config: LoopConfig, llm: Arc<dyn LlmClient>, worktree: PathBuf) -> Self {
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
            worktree,
            iteration: 0,
            status: LoopStatus::Running,
            handlebars: Handlebars::new(),
        }
    }

    /// Run the loop until completion or max iterations
    pub async fn run(&mut self) -> eyre::Result<IterationResult> {
        info!(
            "Starting loop {} (type: {}, max_iterations: {})",
            self.exec_id, self.config.loop_type, self.config.max_iterations
        );

        while self.iteration < self.config.max_iterations {
            self.iteration += 1;
            info!(
                "Loop {} iteration {}/{}",
                self.exec_id, self.iteration, self.config.max_iterations
            );

            let result = self.run_iteration().await?;

            match result {
                IterationResult::Complete { .. } => {
                    self.status = LoopStatus::Complete;
                    return Ok(result);
                }
                IterationResult::Continue { .. } => {
                    // Continue to next iteration
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                IterationResult::RateLimited { retry_after } => {
                    warn!("Rate limited, sleeping for {:?}", retry_after);
                    tokio::time::sleep(retry_after).await;
                    self.iteration -= 1; // Don't count this iteration
                }
                IterationResult::Interrupted { reason } => {
                    self.status = LoopStatus::Stopped;
                    return Ok(IterationResult::Interrupted { reason });
                }
                IterationResult::Error { message, recoverable } => {
                    if !recoverable {
                        self.status = LoopStatus::Failed {
                            reason: message.clone(),
                        };
                        return Ok(IterationResult::Error { message, recoverable });
                    }
                    warn!("Recoverable error: {}", message);
                }
            }
        }

        self.status = LoopStatus::Failed {
            reason: "Max iterations exceeded".to_string(),
        };
        Ok(IterationResult::Error {
            message: format!("Max iterations ({}) exceeded", self.config.max_iterations),
            recoverable: false,
        })
    }

    /// Run a single iteration
    async fn run_iteration(&mut self) -> eyre::Result<IterationResult> {
        // Build context for template
        let context = self.build_template_context().await?;

        // Render prompt
        let prompt = self.render_prompt(&context)?;

        // Create tool context for this iteration
        let tool_ctx = ToolContext::new(self.worktree.clone(), self.exec_id.clone());
        tool_ctx.clear_reads().await;

        // Get tool definitions for this loop type
        let tool_defs = self.tool_executor.definitions_for(&self.config.tools);

        // Run agentic loop (LLM + tool calls until EndTurn)
        let result = self.run_agentic_loop(&prompt, &tool_ctx, &tool_defs).await?;

        match result {
            AgenticLoopResult::Complete => {}
            AgenticLoopResult::RateLimited { retry_after } => {
                return Ok(IterationResult::RateLimited { retry_after });
            }
            AgenticLoopResult::Error { message, recoverable } => {
                return Ok(IterationResult::Error { message, recoverable });
            }
        }

        // Run validation
        let validation = run_validation(
            &self.config.validation_command,
            &self.worktree,
            Duration::from_millis(self.config.iteration_timeout_ms),
        )
        .await?;

        // Record progress
        let files_changed = self.get_changed_files().await;
        let iter_ctx = IterationContext::new(
            self.iteration,
            &self.config.validation_command,
            validation.exit_code,
            &validation.stdout,
            &validation.stderr,
            validation.duration_ms,
            files_changed,
        );
        self.progress.record(&iter_ctx);

        // Check if validation passed
        if validation.passed(self.config.success_exit_code) {
            info!(
                "Loop {} completed successfully after {} iterations",
                self.exec_id, self.iteration
            );
            return Ok(IterationResult::Complete {
                iterations: self.iteration,
            });
        }

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
        &self,
        initial_prompt: &str,
        tool_ctx: &ToolContext,
        tool_defs: &[ToolDefinition],
    ) -> eyre::Result<AgenticLoopResult> {
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

            if turn > self.config.max_turns_per_iteration as usize {
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
                max_tokens: 16384,
            };

            let response = match self.llm.complete(request).await {
                Ok(r) => r,
                Err(e) if e.is_rate_limit() => {
                    return Ok(AgenticLoopResult::RateLimited {
                        retry_after: e.retry_after().unwrap_or(Duration::from_secs(60)),
                    });
                }
                Err(e) if e.is_retryable() => {
                    return Ok(AgenticLoopResult::Error {
                        message: e.to_string(),
                        recoverable: true,
                    });
                }
                Err(e) => {
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
                    // LLM finished its turn
                    break;
                }
                StopReason::ToolUse => {
                    // Execute tools and continue
                    let tool_results = self.execute_tools(&response.tool_calls, tool_ctx).await;

                    // Build user message with tool results
                    let tool_result_message = self.build_tool_result_message(&tool_results);
                    messages.push(tool_result_message);
                }
                StopReason::MaxTokens => {
                    // Output truncated, ask to continue
                    messages.push(Message::user(
                        "Continue from where you left off. Your previous response was truncated.",
                    ));
                }
                StopReason::StopSequence => {
                    break;
                }
            }
        }

        Ok(AgenticLoopResult::Complete)
    }

    /// Execute tool calls and return results
    async fn execute_tools(&self, tool_calls: &[crate::llm::ToolCall], ctx: &ToolContext) -> Vec<(String, ToolResult)> {
        self.tool_executor.execute_all(tool_calls, ctx).await
    }

    /// Build assistant message from response
    fn build_assistant_message(&self, response: &CompletionResponse) -> Message {
        let mut blocks = Vec::new();

        if let Some(text) = &response.content {
            blocks.push(ContentBlock::text(text));
        }

        for call in &response.tool_calls {
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
        let blocks: Vec<ContentBlock> = results
            .iter()
            .map(|(id, result)| ContentBlock::tool_result(id, &result.content, result.is_error))
            .collect();

        Message::user_blocks(blocks)
    }

    /// Build template context for prompt rendering
    async fn build_template_context(&self) -> eyre::Result<HashMap<String, String>> {
        let mut context = HashMap::new();

        // Basic loop info
        context.insert("working-directory".to_string(), self.worktree.display().to_string());
        context.insert("iteration".to_string(), self.iteration.to_string());

        // Git status
        if let Ok(output) = tokio::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.worktree)
            .output()
            .await
        {
            let status = String::from_utf8_lossy(&output.stdout).to_string();
            context.insert("git-status".to_string(), status);
        }

        // Git diff
        if let Ok(output) = tokio::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&self.worktree)
            .output()
            .await
        {
            let diff = String::from_utf8_lossy(&output.stdout).to_string();
            // Truncate large diffs
            let truncated = if diff.len() > 5000 {
                format!("{}...\n[diff truncated]", &diff[..5000])
            } else {
                diff
            };
            context.insert("git-diff".to_string(), truncated);
        }

        // Progress from previous iterations
        context.insert("progress".to_string(), self.progress.get_progress());

        Ok(context)
    }

    /// Render the prompt template with context
    fn render_prompt(&self, context: &HashMap<String, String>) -> eyre::Result<String> {
        // For now, use simple string replacement since Handlebars setup is complex
        let mut result = self.config.prompt_template.clone();

        for (key, value) in context {
            let placeholder = format!("{{{{{}}}}}", key);
            result = result.replace(&placeholder, value);
        }

        Ok(result)
    }

    /// Get list of changed files from git status
    async fn get_changed_files(&self) -> Vec<String> {
        let output = tokio::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.worktree)
            .output()
            .await;

        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    if trimmed.len() > 3 { Some(trimmed[3..].to_string()) } else { None }
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    /// Get current iteration count
    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    /// Get current status
    pub fn status(&self) -> &LoopStatus {
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
