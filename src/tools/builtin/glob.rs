//! glob tool - find files matching a pattern

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use tracing::debug;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Find files matching a glob pattern
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "glob"
    }

    fn description(&self) -> &'static str {
        "Find files matching a glob pattern (e.g., **/*.rs)"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory (default: worktree root)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "GlobTool::execute: called");
        let pattern = match input["pattern"].as_str() {
            Some(p) => {
                debug!(%p, "GlobTool::execute: pattern parameter found");
                p
            }
            None => {
                debug!("GlobTool::execute: missing pattern parameter");
                return ToolResult::error("pattern is required");
            }
        };

        let base = input["path"].as_str().unwrap_or(".");
        debug!(%base, "GlobTool::execute: base path");

        let base_path = match ctx.validate_path(Path::new(base)) {
            Ok(p) => {
                debug!(?p, "GlobTool::execute: base path validated");
                p
            }
            Err(e) => {
                debug!(%e, "GlobTool::execute: base path validation failed");
                return ToolResult::error(e.to_string());
            }
        };

        let full_pattern = base_path.join(pattern);
        let pattern_str = match full_pattern.to_str() {
            Some(s) => {
                debug!(%s, "GlobTool::execute: full pattern string");
                s
            }
            None => {
                debug!("GlobTool::execute: invalid pattern path");
                return ToolResult::error("Invalid pattern path");
            }
        };

        debug!("GlobTool::execute: executing glob");
        let matches: Vec<String> = match glob::glob(pattern_str) {
            Ok(paths) => {
                debug!("GlobTool::execute: glob successful");
                paths
                    .filter_map(|r| r.ok())
                    .filter(|p| {
                        // Sandbox check - ensure path is within worktree
                        p.starts_with(&ctx.worktree)
                    })
                    .filter_map(|p| {
                        p.strip_prefix(&ctx.worktree)
                            .ok()
                            .map(|rel| rel.to_string_lossy().to_string())
                    })
                    .take(1000) // Limit results to prevent huge outputs
                    .collect()
            }
            Err(e) => {
                debug!(%e, "GlobTool::execute: invalid glob pattern");
                return ToolResult::error(format!("Invalid glob pattern: {}", e));
            }
        };

        debug!(matches_count = %matches.len(), "GlobTool::execute: matches found");

        if matches.is_empty() {
            debug!("GlobTool::execute: no matches found");
            ToolResult::success("No matches found")
        } else {
            debug!("GlobTool::execute: returning matches");
            ToolResult::success(matches.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_glob_basic() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("file1.rs"), "").unwrap();
        fs::write(temp.path().join("file2.rs"), "").unwrap();
        fs::write(temp.path().join("file3.txt"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = GlobTool;

        let result = tool.execute(serde_json::json!({"pattern": "*.rs"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("file1.rs"));
        assert!(result.content.contains("file2.rs"));
        assert!(!result.content.contains("file3.txt"));
    }

    #[tokio::test]
    async fn test_glob_recursive() {
        let temp = tempdir().unwrap();
        let subdir = temp.path().join("src");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("lib.rs"), "").unwrap();
        fs::write(temp.path().join("main.rs"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = GlobTool;

        let result = tool.execute(serde_json::json!({"pattern": "**/*.rs"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("main.rs"));
        assert!(result.content.contains("src/lib.rs") || result.content.contains("src\\lib.rs"));
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = GlobTool;

        let result = tool
            .execute(serde_json::json!({"pattern": "*.nonexistent"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("No matches"));
    }

    #[tokio::test]
    async fn test_glob_with_path() {
        let temp = tempdir().unwrap();
        let subdir = temp.path().join("src");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("lib.rs"), "").unwrap();
        fs::write(temp.path().join("main.rs"), "").unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = GlobTool;

        let result = tool
            .execute(serde_json::json!({"pattern": "*.rs", "path": "src"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("lib.rs"));
        // main.rs should not be included since we're searching in src/
    }

    #[tokio::test]
    async fn test_glob_missing_pattern() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = GlobTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("pattern is required"));
    }
}
