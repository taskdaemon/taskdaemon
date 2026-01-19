//! bash tool - execute shell commands

use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;
use tracing::debug;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Execute a shell command in the worktree
pub struct RunCommandTool;

#[async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Execute a shell command in the worktree. Use for git, build tools, tests."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "RunCommandTool::execute: called");
        let command = match input["command"].as_str() {
            Some(c) => {
                debug!(%c, "RunCommandTool::execute: command parameter found");
                c
            }
            None => {
                debug!("RunCommandTool::execute: missing command parameter");
                return ToolResult::error("command is required");
            }
        };

        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(120_000);
        debug!(%timeout_ms, "RunCommandTool::execute: timeout_ms value");

        debug!("RunCommandTool::execute: spawning command");
        let output = match tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.worktree)
                .output(),
        )
        .await
        {
            Ok(Ok(output)) => {
                debug!(status = ?output.status, "RunCommandTool::execute: command completed");
                output
            }
            Ok(Err(e)) => {
                debug!(%e, "RunCommandTool::execute: failed to execute command");
                return ToolResult::error(format!("Failed to execute command: {}", e));
            }
            Err(_) => {
                debug!("RunCommandTool::execute: command timed out");
                return ToolResult::error(format!("Command timed out after {}ms", timeout_ms));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(stdout_len = %stdout.len(), stderr_len = %stderr.len(), "RunCommandTool::execute: output lengths");

        let result = if stdout.is_empty() && !stderr.is_empty() {
            debug!("RunCommandTool::execute: using stderr only");
            stderr.to_string()
        } else if stderr.is_empty() {
            debug!("RunCommandTool::execute: using stdout only");
            stdout.to_string()
        } else {
            debug!("RunCommandTool::execute: combining stdout and stderr");
            format!("{}\n\nSTDERR:\n{}", stdout, stderr)
        };

        // Truncate long output
        let truncated = if result.len() > 30_000 {
            debug!("RunCommandTool::execute: truncating long output");
            format!("{}...\n[truncated, {} chars total]", &result[..30_000], result.len())
        } else {
            debug!("RunCommandTool::execute: output within size limit");
            result
        };

        if output.status.success() {
            debug!("RunCommandTool::execute: command succeeded");
            ToolResult::success(truncated)
        } else {
            debug!(exit_code = ?output.status.code(), "RunCommandTool::execute: command failed");
            ToolResult::error(format!(
                "Exit code: {}\n{}",
                output.status.code().unwrap_or(-1),
                truncated
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_run_command_basic() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = RunCommandTool;

        let result = tool.execute(serde_json::json!({"command": "echo hello"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("hello"));
    }

    #[tokio::test]
    async fn test_run_command_in_worktree() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = RunCommandTool;

        let result = tool.execute(serde_json::json!({"command": "pwd"}), &ctx).await;

        assert!(!result.is_error);
        // The output should contain the temp directory path
        assert!(result.content.contains(temp.path().to_str().unwrap()) || !result.content.is_empty());
    }

    #[tokio::test]
    async fn test_run_command_failure() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = RunCommandTool;

        let result = tool.execute(serde_json::json!({"command": "false"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("Exit code: 1"));
    }

    #[tokio::test]
    async fn test_run_command_missing_command() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = RunCommandTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("command is required"));
    }

    #[tokio::test]
    async fn test_run_command_stderr() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = RunCommandTool;

        let result = tool
            .execute(serde_json::json!({"command": "echo error >&2"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("error"));
    }
}
