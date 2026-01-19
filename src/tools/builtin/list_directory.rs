//! list tool - list files and directories

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use tracing::debug;

use crate::tools::{Tool, ToolContext, ToolResult};

/// List files and directories in a path
pub struct ListDirectoryTool;

#[async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List files and directories in a path."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to worktree (default: .)"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "ListDirectoryTool::execute: called");
        let path = input["path"].as_str().unwrap_or(".");
        debug!(%path, "ListDirectoryTool::execute: path parameter");

        let full_path = match ctx.validate_path(Path::new(path)) {
            Ok(p) => {
                debug!(?p, "ListDirectoryTool::execute: path validated");
                p
            }
            Err(e) => {
                debug!(%e, "ListDirectoryTool::execute: path validation failed");
                return ToolResult::error(e.to_string());
            }
        };

        let mut entries = Vec::new();
        let mut dir = match tokio::fs::read_dir(&full_path).await {
            Ok(d) => {
                debug!("ListDirectoryTool::execute: directory opened");
                d
            }
            Err(e) => {
                debug!(%e, "ListDirectoryTool::execute: failed to read directory");
                return ToolResult::error(format!("Failed to read directory: {}", e));
            }
        };

        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => {
                    debug!(%name, "ListDirectoryTool::execute: failed to get metadata, skipping entry");
                    continue;
                }
            };

            let suffix = if metadata.is_dir() { "/" } else { "" };
            entries.push(format!("{}{}", name, suffix));
        }

        entries.sort();
        debug!(entries_count = %entries.len(), "ListDirectoryTool::execute: entries collected");

        if entries.is_empty() {
            debug!("ListDirectoryTool::execute: empty directory");
            ToolResult::success("(empty directory)")
        } else {
            debug!("ListDirectoryTool::execute: returning entries");
            ToolResult::success(entries.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_list_directory_basic() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("file1.txt"), "").unwrap();
        fs::write(temp.path().join("file2.txt"), "").unwrap();
        fs::create_dir(temp.path().join("subdir")).unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ListDirectoryTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("file1.txt"));
        assert!(result.content.contains("file2.txt"));
        assert!(result.content.contains("subdir/"));
    }

    #[tokio::test]
    async fn test_list_directory_with_path() {
        let temp = tempdir().unwrap();
        let subdir = temp.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("nested.txt"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ListDirectoryTool;

        let result = tool.execute(serde_json::json!({"path": "subdir"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("nested.txt"));
    }

    #[tokio::test]
    async fn test_list_directory_empty() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ListDirectoryTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("empty"));
    }

    #[tokio::test]
    async fn test_list_directory_not_found() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ListDirectoryTool;

        let result = tool.execute(serde_json::json!({"path": "nonexistent"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("Failed to read"));
    }
}
