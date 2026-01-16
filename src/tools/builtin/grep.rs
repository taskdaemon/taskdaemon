//! Grep tool - search files using ripgrep

use std::process::Stdio;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Grep tool - search for patterns in files using ripgrep
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search for patterns in files using ripgrep. Returns matching lines with context."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Path to search in (relative to worktree, default: '.')",
                    "default": "."
                },
                "file_pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g., '*.rs', '*.py')"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines before and after match (default: 2)",
                    "default": 2
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case-insensitive search (default: false)",
                    "default": false
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 50)",
                    "default": 50
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        // Extract parameters
        let pattern = match input.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required parameter: pattern"),
        };

        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let file_pattern = input.get("file_pattern").and_then(|v| v.as_str());

        let context_lines = input.get("context_lines").and_then(|v| v.as_u64()).unwrap_or(2) as usize;

        let case_insensitive = input.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);

        let max_results = input.get("max_results").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        // Validate path is within worktree
        let search_path = match ctx.validate_path(std::path::Path::new(path)) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid path: {}", e)),
        };

        // Check if ripgrep is available
        let rg_check = Command::new("which").arg("rg").output().await;

        if rg_check.is_err() || !rg_check.unwrap().status.success() {
            return ToolResult::error(
                "ripgrep (rg) not found. Install with: brew install ripgrep (macOS) or apt install ripgrep (Linux)",
            );
        }

        // Build ripgrep command
        let mut cmd = Command::new("rg");

        // Basic options
        cmd.arg("--line-number").arg("--no-heading").arg("--color=never");

        // Context lines
        if context_lines > 0 {
            cmd.arg(format!("-C{}", context_lines));
        }

        // Case insensitive
        if case_insensitive {
            cmd.arg("-i");
        }

        // Max results
        cmd.arg(format!("--max-count={}", max_results));

        // File pattern filter
        if let Some(fp) = file_pattern {
            cmd.arg("--glob").arg(fp);
        }

        // Pattern and path
        cmd.arg(pattern);
        cmd.arg(&search_path);

        // Execute
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&ctx.worktree);

        let output = match cmd.output().await {
            Ok(o) => o,
            Err(e) => return ToolResult::error(format!("Failed to execute ripgrep: {}", e)),
        };

        // Check exit status
        match output.status.code() {
            Some(0) => {
                // Found matches
                let stdout = String::from_utf8_lossy(&output.stdout);
                let result = truncate_output(&stdout, max_results);
                ToolResult::success(result)
            }
            Some(1) => {
                // No matches found
                ToolResult::success("No matches found.")
            }
            Some(2) => {
                // Error
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolResult::error(format!("ripgrep error: {}", stderr))
            }
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolResult::error(format!("ripgrep failed: {}", stderr))
            }
        }
    }
}

/// Truncate output to max_results matches
fn truncate_output(output: &str, max_results: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();

    // Count actual match lines (not context lines starting with -)
    let mut match_count = 0;
    let mut include_until = lines.len();

    for (i, line) in lines.iter().enumerate() {
        // Match lines have format: file:line_number:content
        if line.contains(':') && !line.starts_with('-') && !line.starts_with("--") {
            match_count += 1;
            if match_count >= max_results {
                // Include a few more lines for context, then cut off
                include_until = (i + 5).min(lines.len());
                break;
            }
        }
    }

    let result: String = lines[..include_until].join("\n");

    if include_until < lines.len() {
        format!("{}\n\n... (truncated, {} matches shown)", result, max_results)
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_grep_basic() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        // Create test file
        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "hello world\nfoo bar\nhello again")
            .await
            .unwrap();

        let input = json!({
            "pattern": "hello",
            "path": "."
        });

        let tool = GrepTool;
        let result = tool.execute(input, &ctx).await;

        // Note: This test requires ripgrep to be installed
        // If rg is not installed, the result will be an error
        if !result.is_error || !result.content.contains("ripgrep (rg) not found") {
            assert!(result.content.contains("hello") || result.content.contains("No matches"));
        }
    }

    #[test]
    fn test_truncate_output() {
        let output = "file.rs:1:match1\nfile.rs:2:match2\nfile.rs:3:match3";
        let truncated = truncate_output(output, 2);
        assert!(truncated.contains("match1"));
        assert!(truncated.contains("match2"));
    }
}
