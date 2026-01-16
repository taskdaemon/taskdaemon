//! Query tool - inter-ralph query/reply communication

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext, ToolResult};

/// Query tool - send a question to another ralph and wait for a response
pub struct QueryTool;

#[async_trait]
impl Tool for QueryTool {
    fn name(&self) -> &'static str {
        "query"
    }

    fn description(&self) -> &'static str {
        "Query another ralph for information. Sends a question and waits for a response."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_exec_id": {
                    "type": "string",
                    "description": "The execution ID of the ralph to query"
                },
                "question": {
                    "type": "string",
                    "description": "The question to ask the target ralph"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 30000)",
                    "default": 30000
                }
            },
            "required": ["target_exec_id", "question"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        // Check for coordinator
        let coordinator = match &ctx.coordinator {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "Coordination not enabled for this execution. \
                    Query tool requires a coordinator handle to be configured.",
                );
            }
        };

        // Extract parameters
        let target_exec_id = match input.get("target_exec_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return ToolResult::error("Missing required parameter: target_exec_id"),
        };

        let question = match input.get("question").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => return ToolResult::error("Missing required parameter: question"),
        };

        let timeout_ms = input.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(30000);

        let timeout = Duration::from_millis(timeout_ms);

        tracing::debug!(
            from = %ctx.exec_id,
            to = %target_exec_id,
            question = %question,
            timeout_ms = %timeout_ms,
            "Sending query"
        );

        // Send the query and wait for response
        match coordinator.query(target_exec_id, question, timeout).await {
            Ok(answer) => {
                tracing::debug!(
                    from = %ctx.exec_id,
                    to = %target_exec_id,
                    answer_len = %answer.len(),
                    "Received query response"
                );
                ToolResult::success(answer)
            }
            Err(e) => {
                tracing::warn!(
                    from = %ctx.exec_id,
                    to = %target_exec_id,
                    error = %e,
                    "Query failed"
                );
                ToolResult::error(format!("Query failed: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_query_no_coordinator() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "target_exec_id": "other-exec",
            "question": "What is your status?"
        });

        let tool = QueryTool;
        let result = tool.execute(input, &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("Coordination not enabled"));
    }

    #[tokio::test]
    async fn test_query_missing_target() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "question": "What is your status?"
        });

        let tool = QueryTool;
        let result = tool.execute(input, &ctx).await;

        // Without coordinator, fails on coordination check first
        assert!(result.is_error);
        assert!(result.content.contains("Coordination not enabled"));
    }

    #[tokio::test]
    async fn test_query_missing_question() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "target_exec_id": "other-exec"
        });

        let tool = QueryTool;
        let result = tool.execute(input, &ctx).await;

        // Without coordinator, fails on coordination check first
        assert!(result.is_error);
        assert!(result.content.contains("Coordination not enabled"));
    }
}
