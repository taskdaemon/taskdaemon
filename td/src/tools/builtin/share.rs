//! Share tool - inter-ralph data sharing

use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::debug;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Share tool - send data to another ralph for use in its next iteration
pub struct ShareTool;

#[async_trait]
impl Tool for ShareTool {
    fn name(&self) -> &'static str {
        "share"
    }

    fn description(&self) -> &'static str {
        "Share data with another ralph. The target ralph can access this in its next iteration."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_exec_id": {
                    "type": "string",
                    "description": "The execution ID of the ralph to share with"
                },
                "share_type": {
                    "type": "string",
                    "description": "Type of data being shared (e.g., 'api_schema', 'test_results')"
                },
                "data": {
                    "type": "string",
                    "description": "The data to share (typically JSON or text)"
                }
            },
            "required": ["target_exec_id", "share_type", "data"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "ShareTool::execute: called");
        // Check for coordinator
        let coordinator = match &ctx.coordinator {
            Some(c) => {
                debug!("ShareTool::execute: coordinator found");
                c
            }
            None => {
                debug!("ShareTool::execute: no coordinator configured");
                return ToolResult::error(
                    "Coordination not enabled for this execution. \
                    Share tool requires a coordinator handle to be configured.",
                );
            }
        };

        // Extract parameters
        let target_exec_id = match input.get("target_exec_id").and_then(|v| v.as_str()) {
            Some(id) => {
                debug!(%id, "ShareTool::execute: target_exec_id parameter found");
                id
            }
            None => {
                debug!("ShareTool::execute: missing target_exec_id parameter");
                return ToolResult::error("Missing required parameter: target_exec_id");
            }
        };

        let share_type = match input.get("share_type").and_then(|v| v.as_str()) {
            Some(t) => {
                debug!(%t, "ShareTool::execute: share_type parameter found");
                t
            }
            None => {
                debug!("ShareTool::execute: missing share_type parameter");
                return ToolResult::error("Missing required parameter: share_type");
            }
        };

        let data = match input.get("data").and_then(|v| v.as_str()) {
            Some(d) => {
                debug!(data_len = %d.len(), "ShareTool::execute: data parameter found");
                d
            }
            None => {
                debug!("ShareTool::execute: missing data parameter");
                return ToolResult::error("Missing required parameter: data");
            }
        };

        debug!(
            from = %ctx.exec_id,
            to = %target_exec_id,
            share_type = %share_type,
            data_len = %data.len(),
            "ShareTool::execute: sharing data"
        );

        // Try to parse data as JSON for better serialization, fallback to string
        let json_data: Value = serde_json::from_str(data).unwrap_or_else(|_| json!(data));
        debug!("ShareTool::execute: data parsed as JSON");

        // Send the share
        match coordinator.share(target_exec_id, share_type, json_data).await {
            Ok(()) => {
                debug!(
                    from = %ctx.exec_id,
                    to = %target_exec_id,
                    share_type = %share_type,
                    "ShareTool::execute: data shared successfully"
                );
                ToolResult::success(format!(
                    "Successfully shared {} data with {}",
                    share_type, target_exec_id
                ))
            }
            Err(e) => {
                debug!(
                    from = %ctx.exec_id,
                    to = %target_exec_id,
                    error = %e,
                    "ShareTool::execute: share failed"
                );
                tracing::warn!(
                    from = %ctx.exec_id,
                    to = %target_exec_id,
                    error = %e,
                    "Share failed"
                );
                ToolResult::error(format!("Share failed: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_share_no_coordinator() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "target_exec_id": "other-exec",
            "share_type": "api_schema",
            "data": "{\"endpoints\": []}"
        });

        let tool = ShareTool;
        let result = tool.execute(input, &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("Coordination not enabled"));
    }

    #[tokio::test]
    async fn test_share_missing_target() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "share_type": "api_schema",
            "data": "some data"
        });

        let tool = ShareTool;
        let result = tool.execute(input, &ctx).await;

        // Without coordinator, fails on coordination check first
        assert!(result.is_error);
        assert!(result.content.contains("Coordination not enabled"));
    }

    #[tokio::test]
    async fn test_share_missing_type() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "target_exec_id": "other-exec",
            "data": "some data"
        });

        let tool = ShareTool;
        let result = tool.execute(input, &ctx).await;

        // Without coordinator, fails on coordination check first
        assert!(result.is_error);
        assert!(result.content.contains("Coordination not enabled"));
    }

    #[tokio::test]
    async fn test_share_missing_data() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "target_exec_id": "other-exec",
            "share_type": "api_schema"
        });

        let tool = ShareTool;
        let result = tool.execute(input, &ctx).await;

        // Without coordinator, fails on coordination check first
        assert!(result.is_error);
        assert!(result.content.contains("Coordination not enabled"));
    }
}
