//! tree tool - display directory structure as a tree

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use walkdir::WalkDir;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Display directory structure as a tree
pub struct TreeTool;

#[async_trait]
impl Tool for TreeTool {
    fn name(&self) -> &'static str {
        "tree"
    }

    fn description(&self) -> &'static str {
        "Display directory structure as a tree"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path (default: current directory)"
                },
                "depth": {
                    "type": "integer",
                    "description": "Maximum depth to traverse (default: 3)"
                },
                "show_hidden": {
                    "type": "boolean",
                    "description": "Show hidden files (default: false)"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let path = input["path"].as_str().unwrap_or(".");
        let depth = input["depth"].as_u64().unwrap_or(3) as usize;
        let show_hidden = input["show_hidden"].as_bool().unwrap_or(false);

        let full_path = match ctx.validate_path(Path::new(path)) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(e.to_string()),
        };

        if !full_path.is_dir() {
            return ToolResult::error(format!("{} is not a directory", path));
        }

        let mut output = Vec::new();

        // Add root directory name
        let root_name = full_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        output.push(format!("{}/", root_name));

        // Walk directory tree
        let root_for_filter = full_path.clone();
        let walker = WalkDir::new(&full_path)
            .max_depth(depth)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(move |e| {
                // Always include the root directory
                if e.path() == root_for_filter {
                    return true;
                }
                if show_hidden {
                    true
                } else {
                    // Skip hidden files/directories (starting with .)
                    e.file_name().to_str().map(|s| !s.starts_with('.')).unwrap_or(true)
                }
            });

        for entry in walker.filter_map(|e| e.ok()) {
            // Skip the root directory itself
            if entry.path() == full_path {
                continue;
            }

            let current_depth = entry.depth();
            let is_dir = entry.file_type().is_dir();
            let name = entry.file_name().to_string_lossy();
            let suffix = if is_dir { "/" } else { "" };

            // Build tree prefix using typical tree characters
            let indent = "    ".repeat(current_depth.saturating_sub(1));
            let connector = if current_depth > 0 { "├── " } else { "" };

            output.push(format!("{}{}{}{}", indent, connector, name, suffix));
        }

        // Limit output size
        let max_lines = 500;
        if output.len() > max_lines {
            output.truncate(max_lines);
            output.push(format!("... (truncated, {} entries total)", output.len()));
        }

        ToolResult::success(output.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tree_basic() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("file1.txt"), "").unwrap();
        fs::write(temp.path().join("file2.txt"), "").unwrap();
        fs::create_dir(temp.path().join("subdir")).unwrap();
        fs::write(temp.path().join("subdir/nested.txt"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TreeTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("file1.txt"));
        assert!(result.content.contains("file2.txt"));
        assert!(result.content.contains("subdir/"));
        assert!(result.content.contains("nested.txt"));
    }

    #[tokio::test]
    async fn test_tree_with_depth() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("a/b/c/d")).unwrap();
        fs::write(temp.path().join("a/b/c/d/deep.txt"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TreeTool;

        // Depth 2 should not show d/ or deep.txt
        let result = tool.execute(serde_json::json!({"depth": 2}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("a/"));
        assert!(result.content.contains("b/"));
        assert!(!result.content.contains("d/"));
        assert!(!result.content.contains("deep.txt"));
    }

    #[tokio::test]
    async fn test_tree_hidden_files() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("visible.txt"), "").unwrap();
        fs::write(temp.path().join(".hidden"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TreeTool;

        // Without show_hidden, should not include .hidden
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.content.contains("visible.txt"));
        assert!(!result.content.contains(".hidden"));

        // With show_hidden, should include .hidden
        let result = tool.execute(serde_json::json!({"show_hidden": true}), &ctx).await;
        assert!(result.content.contains("visible.txt"));
        assert!(result.content.contains(".hidden"));
    }

    #[tokio::test]
    async fn test_tree_not_a_directory() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("file.txt"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TreeTool;

        let result = tool.execute(serde_json::json!({"path": "file.txt"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("not a directory"));
    }
}
