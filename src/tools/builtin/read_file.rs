//! read_file tool - read file contents with line numbers

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Read a file's contents with line numbers
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a file's contents with line numbers. Required before editing."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to worktree"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max lines to read (default: 2000)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let path = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolResult::error("path is required"),
        };

        let offset = input["offset"].as_u64().unwrap_or(1) as usize;
        let limit = input["limit"].as_u64().unwrap_or(2000) as usize;

        let full_path = match ctx.validate_path(Path::new(path)) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(e.to_string()),
        };

        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        // Track read for edit validation
        ctx.track_read(&full_path).await;

        // Format with line numbers (cat -n style)
        let lines: Vec<String> = content
            .lines()
            .skip(offset.saturating_sub(1))
            .take(limit)
            .enumerate()
            .map(|(i, line)| {
                let line_num = offset + i;
                let truncated = if line.len() > 2000 {
                    format!("{}...", &line[..2000])
                } else {
                    line.to_string()
                };
                format!("{:>6}│{}", line_num, truncated)
            })
            .collect();

        ToolResult::success(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_read_file_basic() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "line 1\nline 2\nline 3").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadFileTool;

        let result = tool.execute(serde_json::json!({"path": "test.txt"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("line 1"));
        assert!(result.content.contains("line 2"));
        assert!(result.content.contains("line 3"));
    }

    #[tokio::test]
    async fn test_read_file_with_offset() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "line 1\nline 2\nline 3\nline 4").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadFileTool;

        let result = tool
            .execute(serde_json::json!({"path": "test.txt", "offset": 2}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(!result.content.contains("│line 1")); // Line 1 should be skipped
        assert!(result.content.contains("line 2"));
    }

    #[tokio::test]
    async fn test_read_file_with_limit() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "line 1\nline 2\nline 3\nline 4").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadFileTool;

        let result = tool
            .execute(serde_json::json!({"path": "test.txt", "limit": 2}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("line 1"));
        assert!(result.content.contains("line 2"));
        assert!(!result.content.contains("line 3"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadFileTool;

        let result = tool.execute(serde_json::json!({"path": "nonexistent.txt"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("Failed to read"));
    }

    #[tokio::test]
    async fn test_read_file_tracks_read() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "content").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadFileTool;

        // Before read
        assert!(!ctx.was_read(Path::new("test.txt")).await);

        tool.execute(serde_json::json!({"path": "test.txt"}), &ctx).await;

        // After read
        assert!(ctx.was_read(&file_path).await);
    }
}
