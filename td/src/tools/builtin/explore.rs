//! Explore tool - spawn a read-only exploration agent
//!
//! This tool allows tasks to spawn explore subagents for investigating codebases.
//! The explore agent runs in isolation and returns a summary of its findings.

use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use crate::tools::{ExploreConfig, Thoroughness, Tool, ToolContext, ToolResult};

/// Spawn a read-only exploration agent to investigate the codebase
pub struct ExploreTool;

#[async_trait]
impl Tool for ExploreTool {
    fn name(&self) -> &'static str {
        "explore"
    }

    fn description(&self) -> &'static str {
        "Spawn a read-only exploration agent to investigate the codebase. \
         Returns summarized findings. Use for understanding code structure, \
         finding implementations, or researching patterns. \
         The explore agent has its own context window and cannot modify files."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to investigate about the codebase"
                },
                "thoroughness": {
                    "type": "string",
                    "enum": ["quick", "medium", "thorough"],
                    "default": "medium",
                    "description": "How deep to search: quick (3 iterations), medium (6), thorough (10)"
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "ExploreTool::execute: called");

        // Check if explore spawner is available
        let spawner = match &ctx.explore_spawner {
            Some(s) => s,
            None => {
                debug!("ExploreTool::execute: explore_spawner not available");
                return ToolResult::error(
                    "Explore not available in this context. \
                     This may be because you're already in an explore task (nested explores are disabled) \
                     or the explore capability wasn't configured.",
                );
            }
        };

        // Extract parameters
        let question = match input["question"].as_str() {
            Some(q) if !q.trim().is_empty() => q.to_string(),
            _ => {
                debug!("ExploreTool::execute: missing or empty question");
                return ToolResult::error("question is required and cannot be empty");
            }
        };

        let thoroughness = input["thoroughness"]
            .as_str()
            .and_then(|s| s.parse::<Thoroughness>().ok())
            .unwrap_or_default();

        debug!(
            %question,
            ?thoroughness,
            parent_id = %ctx.exec_id,
            "ExploreTool::execute: spawning explore task"
        );

        // Build explore config
        let config = ExploreConfig {
            question: question.clone(),
            thoroughness,
            parent_id: Some(ctx.exec_id.clone()),
            worktree: ctx.worktree.clone(),
            max_iterations: thoroughness.max_iterations(),
            model: None, // Use default (Haiku)
            timeout_secs: 120,
        };

        // Spawn explore and wait for result
        match spawner.spawn(config).await {
            Ok(summary) => {
                debug!(summary_len = summary.len(), "ExploreTool::execute: explore completed");
                ToolResult::success(format!(
                    "## Exploration Results\n\nQuestion: {}\nThoroughness: {}\n\n{}",
                    question, thoroughness, summary
                ))
            }
            Err(e) => {
                debug!(error = %e, "ExploreTool::execute: explore failed");
                ToolResult::error(format!("Exploration failed: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_explore_tool_name() {
        let tool = ExploreTool;
        assert_eq!(tool.name(), "explore");
    }

    #[test]
    fn test_explore_tool_schema() {
        let tool = ExploreTool;
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["question"].is_object());
        assert!(schema["properties"]["thoroughness"].is_object());
        assert!(schema["required"].as_array().unwrap().contains(&"question".into()));
    }

    #[tokio::test]
    async fn test_explore_tool_no_spawner() {
        let tool = ExploreTool;
        let ctx = ToolContext::new(PathBuf::from("/tmp"), "test".to_string());

        let result = tool
            .execute(serde_json::json!({"question": "What is this?"}), &ctx)
            .await;

        assert!(result.is_error);
        assert!(result.content.contains("not available"));
    }

    #[tokio::test]
    async fn test_explore_tool_empty_question() {
        let tool = ExploreTool;
        let ctx = ToolContext::new(PathBuf::from("/tmp"), "test".to_string());

        // Without spawner, the "not available" error comes first
        let result = tool.execute(serde_json::json!({"question": ""}), &ctx).await;

        assert!(result.is_error);
        // Without an explore_spawner, we get "not available" error before question validation
        assert!(result.content.contains("not available"));
    }

    #[tokio::test]
    async fn test_explore_tool_missing_question() {
        let tool = ExploreTool;
        let ctx = ToolContext::new(PathBuf::from("/tmp"), "test".to_string());

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(result.is_error);
        // Without an explore_spawner, we get "not available" error before question validation
        assert!(result.content.contains("not available"));
    }
}
