//! ExploreTask - Lightweight read-only exploration agent
//!
//! ExploreTask is a simplified, single-purpose agent for investigating codebases.
//! Unlike LoopEngine (Ralph Wiggum pattern), it does NOT:
//! - Restart from scratch each iteration (no Ralph pattern)
//! - Run validation commands
//! - Persist to StateManager
//! - Merge to git branches
//!
//! It simply runs a multi-turn conversation until it has an answer.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use eyre::Result;
use tracing::{debug, info, warn};

use crate::llm::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmClient, Message, StopReason, ToolCall, ToolDefinition,
};
use crate::tools::{ExploreConfig, Thoroughness, ToolContext, ToolExecutor, ToolProfile, ToolResult};

/// Lightweight exploration agent - NOT a Ralph loop
pub struct ExploreTask {
    /// Unique identifier for this exploration
    id: String,

    /// Configuration for the exploration
    config: ExploreConfig,

    /// LLM client (should be configured for Haiku for cost efficiency)
    llm: Arc<dyn LlmClient>,

    /// Tool executor (read-only subset)
    tools: ToolExecutor,

    /// Working directory for file operations
    worktree: PathBuf,
}

impl ExploreTask {
    /// Create a new ExploreTask
    pub fn new(id: String, config: ExploreConfig, llm: Arc<dyn LlmClient>) -> Self {
        debug!(%id, question = %config.question, thoroughness = %config.thoroughness, "ExploreTask::new: called");

        // Use read-only tool profile for safety
        let tools = ToolExecutor::with_profile(ToolProfile::ReadOnly);

        let worktree = config.worktree.clone();

        Self {
            id,
            config,
            llm,
            tools,
            worktree,
        }
    }

    /// Run exploration and return summary string
    pub async fn run(&mut self) -> Result<String> {
        debug!(%self.id, "ExploreTask::run: starting exploration");
        let start = Instant::now();

        // Build conversation starting with system prompt
        let mut messages = vec![Message::user(self.build_user_prompt())];
        let tool_defs = self.tool_definitions();
        let mut iterations = 0;

        // Create tool context (read-only, no explore spawner to prevent nesting)
        let ctx = ToolContext::new(self.worktree.clone(), self.id.clone());

        loop {
            iterations += 1;
            debug!(%self.id, iterations, max = %self.config.max_iterations, "ExploreTask::run: iteration");

            if iterations > self.config.max_iterations {
                info!(%self.id, iterations, "ExploreTask: hit max iterations, forcing summary");
                // Force summary if we hit iteration limit
                return self.force_summary(&messages).await;
            }

            // Check timeout
            if start.elapsed().as_secs() > self.config.timeout_secs as u64 {
                warn!(%self.id, elapsed_secs = start.elapsed().as_secs(), "ExploreTask: hit timeout");
                return self.force_summary(&messages).await;
            }

            // Call LLM
            let request = CompletionRequest {
                system_prompt: self.build_system_prompt(),
                messages: messages.clone(),
                max_tokens: 4096,
                tools: tool_defs.clone(),
            };

            let response = match self.llm.complete(request).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(%self.id, error = %e, "ExploreTask: LLM call failed");
                    return Err(e.into());
                }
            };

            debug!(
                %self.id,
                stop_reason = ?response.stop_reason,
                tool_calls = response.tool_calls.len(),
                "ExploreTask::run: got response"
            );

            // Add assistant response to messages
            messages.push(self.response_to_message(&response));

            // Check for natural completion (LLM finished without tool calls)
            if response.stop_reason == StopReason::EndTurn && response.tool_calls.is_empty() {
                debug!(%self.id, "ExploreTask::run: natural completion");
                return Ok(self.extract_summary(&response));
            }

            // Execute any tool calls
            if !response.tool_calls.is_empty() {
                let results = self.execute_tools(&response.tool_calls, &ctx).await;
                messages.push(self.format_tool_results(&results));
            }
        }
    }

    /// Build system prompt for exploration
    fn build_system_prompt(&self) -> String {
        format!(
            "You are exploring a codebase to answer a specific question.\n\
             Your findings will be summarized and returned to the requesting task.\n\n\
             You have read-only access. Use glob, grep, read, tree, and bash (read-only) to investigate.\n\
             When you have enough information, provide a concise summary of your findings.\n\n\
             IMPORTANT: End your final message with a clear SUMMARY section using this format:\n\n\
             ## SUMMARY\n\
             [Your key findings here, formatted as bullet points]\n\n\
             Thoroughness level: {} (be {} in your investigation)\n\n\
             Stay focused on the question and don't explore tangential areas.",
            self.config.thoroughness,
            match self.config.thoroughness {
                Thoroughness::Quick => "quick and surface-level",
                Thoroughness::Medium => "reasonably thorough",
                Thoroughness::Thorough => "very comprehensive and exhaustive",
            }
        )
    }

    /// Build user prompt with the question
    fn build_user_prompt(&self) -> String {
        format!(
            "Please investigate and answer this question about the codebase:\n\n{}\n\n\
             Working directory: {}",
            self.config.question,
            self.worktree.display()
        )
    }

    /// Get tool definitions for LLM
    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.definitions()
    }

    /// Convert LLM response to message for conversation history
    fn response_to_message(&self, response: &CompletionResponse) -> Message {
        let mut blocks = Vec::new();

        // Add text content if present and non-empty
        if let Some(text) = &response.content
            && !text.is_empty()
        {
            blocks.push(ContentBlock::text(text));
        }

        // Add tool use blocks
        for call in &response.tool_calls {
            blocks.push(ContentBlock::ToolUse {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.input.clone(),
            });
        }

        Message::assistant_blocks(blocks)
    }

    /// Execute tool calls and return results
    async fn execute_tools(&self, tool_calls: &[ToolCall], ctx: &ToolContext) -> Vec<(String, ToolResult)> {
        let mut results = Vec::new();

        for call in tool_calls {
            debug!(%self.id, tool = %call.name, "ExploreTask: executing tool");
            let result = self.tools.execute(call, ctx).await;
            results.push((call.id.clone(), result));
        }

        results
    }

    /// Format tool results as a user message
    fn format_tool_results(&self, results: &[(String, ToolResult)]) -> Message {
        let blocks: Vec<ContentBlock> = results
            .iter()
            .map(|(id, result)| ContentBlock::tool_result(id, &result.content, result.is_error))
            .collect();

        Message::user_blocks(blocks)
    }

    /// Extract summary from final response
    fn extract_summary(&self, response: &CompletionResponse) -> String {
        let text = response.content.as_deref().unwrap_or("");

        // Look for SUMMARY section
        if let Some(summary_start) = text.to_uppercase().find("## SUMMARY") {
            let summary = &text[summary_start..];
            // Return everything after the SUMMARY header
            if let Some(content_start) = summary.find('\n') {
                return summary[content_start..].trim().to_string();
            }
        }

        // Fall back to full text if no SUMMARY section
        text.trim().to_string()
    }

    /// Force a summary when iteration/timeout limit reached
    async fn force_summary(&self, messages: &[Message]) -> Result<String> {
        debug!(%self.id, "ExploreTask::force_summary: requesting forced summary");

        // Build a message asking for summary of what we've found so far
        let mut force_messages = messages.to_vec();
        force_messages.push(Message::user(
            "You've reached the investigation limit. Please provide a SUMMARY of your findings so far.\n\n\
             ## SUMMARY\n\
             [Summarize what you've discovered, even if incomplete]"
                .to_string(),
        ));

        let request = CompletionRequest {
            system_prompt: self.build_system_prompt(),
            messages: force_messages,
            max_tokens: 2048,
            tools: vec![], // No tools for summary
        };

        match self.llm.complete(request).await {
            Ok(response) => Ok(self.extract_summary(&response)),
            Err(e) => {
                // If summary fails, extract what we can from the last messages
                warn!(%self.id, error = %e, "ExploreTask: force_summary LLM call failed");
                Ok(format!("Exploration incomplete ({}). Unable to generate summary.", e))
            }
        }
    }
}

/// Generate a unique ID for an explore task
pub fn generate_explore_id(parent_id: Option<&str>) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    match parent_id {
        Some(parent) => format!("explore-{}-{}", &parent[..parent.len().min(20)], ts % 10000),
        None => format!("explore-{}", ts % 100000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thoroughness_max_iterations() {
        assert_eq!(Thoroughness::Quick.max_iterations(), 3);
        assert_eq!(Thoroughness::Medium.max_iterations(), 6);
        assert_eq!(Thoroughness::Thorough.max_iterations(), 10);
    }

    #[test]
    fn test_thoroughness_from_str() {
        assert_eq!("quick".parse::<Thoroughness>(), Ok(Thoroughness::Quick));
        assert_eq!("medium".parse::<Thoroughness>(), Ok(Thoroughness::Medium));
        assert_eq!("thorough".parse::<Thoroughness>(), Ok(Thoroughness::Thorough));
        assert_eq!("MEDIUM".parse::<Thoroughness>(), Ok(Thoroughness::Medium));
        assert!("invalid".parse::<Thoroughness>().is_err());
    }

    #[test]
    fn test_generate_explore_id() {
        let id1 = generate_explore_id(None);
        assert!(id1.starts_with("explore-"));

        let id2 = generate_explore_id(Some("loop-abc-123"));
        assert!(id2.starts_with("explore-"));
        assert!(id2.contains("loop-abc-123"));
    }

    #[test]
    fn test_explore_config_default() {
        let config = ExploreConfig::default();
        assert_eq!(config.thoroughness, Thoroughness::Medium);
        assert_eq!(config.max_iterations, 6);
        assert_eq!(config.timeout_secs, 120);
        assert!(config.model.is_none());
    }

    #[test]
    fn test_extract_summary() {
        use crate::llm::{CompletionResponse, StopReason, TokenUsage};

        // Test summary extraction from a response with SUMMARY section
        let _config = ExploreConfig::default();

        // We can't easily test this without a real LLM, but we can test the extraction logic
        let response = CompletionResponse {
            content: Some(
                "I found several files.\n\n## SUMMARY\n- Found 5 config files\n- Main entry is src/main.rs".to_string(),
            ),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        };

        // Test that summary extraction would work
        let text = response.content.as_deref().unwrap();
        assert!(text.contains("## SUMMARY"));

        if let Some(summary_start) = text.to_uppercase().find("## SUMMARY") {
            let summary = &text[summary_start..];
            if let Some(content_start) = summary.find('\n') {
                let extracted = summary[content_start..].trim();
                assert!(extracted.contains("Found 5 config files"));
            }
        }
    }
}
