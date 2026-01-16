//! CompleteTask tool - signal task completion

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext, ToolResult};

/// CompleteTask tool - signal that the current task is complete
pub struct CompleteTaskTool;

#[async_trait]
impl Tool for CompleteTaskTool {
    fn name(&self) -> &'static str {
        "complete_task"
    }

    fn description(&self) -> &'static str {
        "Signal that the current task is complete. Use when validation passes and work is done."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Brief summary of what was accomplished"
                },
                "artifacts": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of files created or modified"
                }
            },
            "required": ["summary"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let summary = match input.get("summary").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing required parameter: summary"),
        };

        let artifacts: Vec<String> = input
            .get("artifacts")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        // Log the completion (for debugging/tracing)
        tracing::info!(
            exec_id = %ctx.exec_id,
            summary = %summary,
            artifacts = ?artifacts,
            "Task completion signaled"
        );

        // Build a success message
        let mut message = format!("Task completed: {}", summary);

        if !artifacts.is_empty() {
            message.push_str("\n\nArtifacts:\n");
            for artifact in &artifacts {
                message.push_str(&format!("  - {}\n", artifact));
            }
        }

        // The loop engine will detect this tool call and mark the task as complete
        // This tool itself just returns success
        ToolResult::success(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_complete_task_basic() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "summary": "Implemented the feature"
        });

        let tool = CompleteTaskTool;
        let result = tool.execute(input, &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("Task completed"));
        assert!(result.content.contains("Implemented the feature"));
    }

    #[tokio::test]
    async fn test_complete_task_with_artifacts() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "summary": "Added new module",
            "artifacts": ["src/module.rs", "src/tests.rs"]
        });

        let tool = CompleteTaskTool;
        let result = tool.execute(input, &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("src/module.rs"));
        assert!(result.content.contains("src/tests.rs"));
    }

    #[tokio::test]
    async fn test_complete_task_missing_summary() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({});

        let tool = CompleteTaskTool;
        let result = tool.execute(input, &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("Missing required parameter"));
    }
}
