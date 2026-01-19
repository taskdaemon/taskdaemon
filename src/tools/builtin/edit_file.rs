//! edit tool - replace strings in a file

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use tracing::debug;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Replace a specific string in a file
pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> &'static str {
        "Replace a specific string in a file. Requires prior read call."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to worktree"
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact string to find and replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement string"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "EditFileTool::execute: called");
        let path = match input["path"].as_str() {
            Some(p) => {
                debug!(%p, "EditFileTool::execute: path parameter found");
                p
            }
            None => {
                debug!("EditFileTool::execute: missing path parameter");
                return ToolResult::error("path is required");
            }
        };

        let old_string = match input["old_string"].as_str() {
            Some(s) => {
                debug!("EditFileTool::execute: old_string parameter found");
                s
            }
            None => {
                debug!("EditFileTool::execute: missing old_string parameter");
                return ToolResult::error("old_string is required");
            }
        };

        let new_string = match input["new_string"].as_str() {
            Some(s) => {
                debug!("EditFileTool::execute: new_string parameter found");
                s
            }
            None => {
                debug!("EditFileTool::execute: missing new_string parameter");
                return ToolResult::error("new_string is required");
            }
        };

        let replace_all = input["replace_all"].as_bool().unwrap_or(false);
        debug!(%replace_all, "EditFileTool::execute: replace_all value");

        let full_path = match ctx.validate_path(Path::new(path)) {
            Ok(p) => {
                debug!(?p, "EditFileTool::execute: path validated");
                p
            }
            Err(e) => {
                debug!(%e, "EditFileTool::execute: path validation failed");
                return ToolResult::error(e.to_string());
            }
        };

        // Must read file first
        if !ctx.was_read(&full_path).await {
            debug!("EditFileTool::execute: file not read before editing");
            return ToolResult::error("Must read before editing. Read the file first to see current content.");
        }

        debug!("EditFileTool::execute: file was read, proceeding with edit");

        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => {
                debug!(content_len = %c.len(), "EditFileTool::execute: file content read");
                c
            }
            Err(e) => {
                debug!(%e, "EditFileTool::execute: failed to read file");
                return ToolResult::error(format!("Failed to read file: {}", e));
            }
        };

        // Verify old_string exists
        if !content.contains(old_string) {
            debug!("EditFileTool::execute: old_string not found in file");
            return ToolResult::error(
                "old_string not found in file. Make sure it matches exactly including whitespace.",
            );
        }

        debug!("EditFileTool::execute: old_string found in file");

        // Verify uniqueness (unless replace_all)
        if !replace_all {
            let count = content.matches(old_string).count();
            debug!(%count, "EditFileTool::execute: old_string occurrence count");
            if count > 1 {
                debug!("EditFileTool::execute: multiple occurrences found without replace_all");
                return ToolResult::error(format!(
                    "old_string found {} times. Use replace_all=true or provide more context.",
                    count
                ));
            }
        }

        let replacement_count = content.matches(old_string).count();

        let new_content = if replace_all {
            debug!("EditFileTool::execute: replacing all occurrences");
            content.replace(old_string, new_string)
        } else {
            debug!("EditFileTool::execute: replacing first occurrence only");
            content.replacen(old_string, new_string, 1)
        };

        if let Err(e) = tokio::fs::write(&full_path, &new_content).await {
            debug!(%e, "EditFileTool::execute: failed to write file");
            return ToolResult::error(format!("Failed to write file: {}", e));
        }

        debug!("EditFileTool::execute: file written successfully");

        let replacements = if replace_all { replacement_count } else { 1 };

        ToolResult::success(format!("Replaced {} occurrence(s) in {}", replacements, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    async fn setup_and_read(temp: &tempfile::TempDir, filename: &str, content: &str) -> ToolContext {
        let file_path = temp.path().join(filename);
        fs::write(&file_path, content).unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());

        // Simulate reading the file first
        ctx.track_read(&file_path).await;

        ctx
    }

    #[tokio::test]
    async fn test_edit_file_basic() {
        let temp = tempdir().unwrap();
        let ctx = setup_and_read(&temp, "test.txt", "hello world").await;
        let tool = EditFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "test.txt",
                    "old_string": "world",
                    "new_string": "rust"
                }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);

        let content = fs::read_to_string(temp.path().join("test.txt")).unwrap();
        assert_eq!(content, "hello rust");
    }

    #[tokio::test]
    async fn test_edit_file_without_read() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        // Don't read the file first
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = EditFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "test.txt",
                    "old_string": "world",
                    "new_string": "rust"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_error);
        assert!(result.content.contains("Must read before editing"));
    }

    #[tokio::test]
    async fn test_edit_file_pattern_not_found() {
        let temp = tempdir().unwrap();
        let ctx = setup_and_read(&temp, "test.txt", "hello world").await;
        let tool = EditFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "test.txt",
                    "old_string": "notfound",
                    "new_string": "replacement"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_edit_file_multiple_occurrences_without_replace_all() {
        let temp = tempdir().unwrap();
        let ctx = setup_and_read(&temp, "test.txt", "hello hello hello").await;
        let tool = EditFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "test.txt",
                    "old_string": "hello",
                    "new_string": "hi"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_error);
        assert!(result.content.contains("3 times"));
        assert!(result.content.contains("replace_all"));
    }

    #[tokio::test]
    async fn test_edit_file_replace_all() {
        let temp = tempdir().unwrap();
        let ctx = setup_and_read(&temp, "test.txt", "hello hello hello").await;
        let tool = EditFileTool;

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "test.txt",
                    "old_string": "hello",
                    "new_string": "hi",
                    "replace_all": true
                }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("3 occurrence"));

        let content = fs::read_to_string(temp.path().join("test.txt")).unwrap();
        assert_eq!(content, "hi hi hi");
    }
}
