//! write tool - write content to a file

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use tracing::debug;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Write content to a file
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Write content to a file. Creates parent directories if needed."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to worktree"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "WriteFileTool::execute: called");
        let path = match input["path"].as_str() {
            Some(p) => {
                debug!(%p, "WriteFileTool::execute: path parameter found");
                p
            }
            None => {
                debug!("WriteFileTool::execute: missing path parameter");
                return ToolResult::error("path is required");
            }
        };

        let content = match input["content"].as_str() {
            Some(c) => {
                debug!(content_len = %c.len(), "WriteFileTool::execute: content parameter found");
                c
            }
            None => {
                debug!("WriteFileTool::execute: missing content parameter");
                return ToolResult::error("content is required");
            }
        };

        let full_path = match ctx.validate_path(Path::new(path)) {
            Ok(p) => {
                debug!(?p, "WriteFileTool::execute: path validated");
                p
            }
            Err(e) => {
                debug!(%e, "WriteFileTool::execute: path validation failed");
                return ToolResult::error(e.to_string());
            }
        };

        // Create parent directories
        if let Some(parent) = full_path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            debug!(%e, "WriteFileTool::execute: failed to create parent directories");
            return ToolResult::error(format!("Failed to create directories: {}", e));
        }

        debug!("WriteFileTool::execute: parent directories ensured");

        if let Err(e) = tokio::fs::write(&full_path, content).await {
            debug!(%e, "WriteFileTool::execute: failed to write file");
            return ToolResult::error(format!("Failed to write file: {}", e));
        }

        // Track as read so edit_file can be used immediately after write
        ctx.track_read(&full_path).await;

        debug!(bytes = %content.len(), "WriteFileTool::execute: file written successfully");
        ToolResult::success(format!("Wrote {} bytes to {}", content.len(), path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_write_file_basic() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = WriteFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "test.txt",
                    "content": "Hello, world!"
                }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("13 bytes"));

        let content = fs::read_to_string(temp.path().join("test.txt")).unwrap();
        assert_eq!(content, "Hello, world!");
    }

    #[tokio::test]
    async fn test_write_file_creates_directories() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = WriteFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "nested/dir/test.txt",
                    "content": "content"
                }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);

        let content = fs::read_to_string(temp.path().join("nested/dir/test.txt")).unwrap();
        assert_eq!(content, "content");
    }

    #[tokio::test]
    async fn test_write_file_overwrites_existing() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "old content").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = WriteFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "test.txt",
                    "content": "new content"
                }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_write_file_missing_content() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = WriteFileTool;

        let result = tool.execute(serde_json::json!({"path": "test.txt"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("content is required"));
    }
}
