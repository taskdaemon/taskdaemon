//! Read-only bash tool - execute shell commands with write operations blocked
//!
//! This is a restricted version of the bash tool for use in read-only contexts
//! like the Explore agent. It blocks commands that could modify the filesystem.

use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;
use tracing::debug;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Blocked commands and patterns for read-only mode
const BLOCKED_COMMANDS: &[&str] = &[
    // File modification commands
    "rm",
    "rmdir",
    "mv",
    "cp",
    "touch",
    "mkdir",
    "chmod",
    "chown",
    "chgrp",
    "truncate",
    "shred",
    // Text editors (would create/modify files)
    "vim",
    "vi",
    "nano",
    "emacs",
    "ed",
    // Git write operations
    "git push",
    "git reset",
    "git checkout",
    "git clean",
    "git stash",
    "git rebase",
    "git merge",
    "git commit",
    "git add",
    "git rm",
    "git mv",
    "git restore",
    "git cherry-pick",
    // Package managers (could modify system)
    "apt",
    "apt-get",
    "yum",
    "dnf",
    "brew",
    "npm install",
    "npm uninstall",
    "pip install",
    "pip uninstall",
    "cargo install",
    // Other dangerous commands
    "dd",
    "mkfs",
    "wget -O",
    "curl -O",
    "curl --output",
];

/// Blocked output redirections
const BLOCKED_REDIRECTS: &[&str] = &[
    ">", // Output redirect (overwrites)
    ">>", // Output redirect (appends)
         // Note: We don't block < or | as those are read operations
];

/// Execute a shell command in the worktree with read-only restrictions
pub struct ReadOnlyBashTool;

impl ReadOnlyBashTool {
    /// Check if a command contains any blocked patterns
    fn is_blocked(command: &str) -> Option<&'static str> {
        let command_lower = command.to_lowercase();

        // Check for output redirections first (highest priority)
        for redirect in BLOCKED_REDIRECTS {
            // Look for redirect that's not escaped
            if command.contains(redirect) {
                // Make sure it's not in a string like "grep '>'" or part of another pattern
                let parts: Vec<&str> = command.split_whitespace().collect();
                for part in parts {
                    if part.contains(redirect) && !part.starts_with('\'') && !part.starts_with('"') {
                        return Some(redirect);
                    }
                }
            }
        }

        // Check for blocked commands
        for blocked in BLOCKED_COMMANDS {
            // Check if command starts with blocked command or contains it after a pipe/semicolon
            if command_lower.starts_with(blocked)
                || command_lower.starts_with(&format!("{} ", blocked))
                || command_lower.contains(&format!(" {}", blocked))
                || command_lower.contains(&format!(";{}", blocked))
                || command_lower.contains(&format!("; {}", blocked))
                || command_lower.contains(&format!("|{}", blocked))
                || command_lower.contains(&format!("| {}", blocked))
                || command_lower.contains(&format!("&&{}", blocked))
                || command_lower.contains(&format!("&& {}", blocked))
            {
                return Some(blocked);
            }
        }

        None
    }
}

#[async_trait]
impl Tool for ReadOnlyBashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Execute a read-only shell command in the worktree. Write operations are blocked. \
         Use for inspecting files, running git log/status/diff, and other read operations."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute (write operations will be blocked)"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 60000)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "ReadOnlyBashTool::execute: called");
        let command = match input["command"].as_str() {
            Some(c) => {
                debug!(%c, "ReadOnlyBashTool::execute: command parameter found");
                c
            }
            None => {
                debug!("ReadOnlyBashTool::execute: missing command parameter");
                return ToolResult::error("command is required");
            }
        };

        // Check if command is blocked
        if let Some(blocked) = Self::is_blocked(command) {
            debug!(%blocked, "ReadOnlyBashTool::execute: command blocked");
            return ToolResult::error(format!(
                "Command blocked in read-only mode: '{}' is not allowed. \
                 This bash tool only allows read operations.",
                blocked
            ));
        }

        // Shorter default timeout for exploration (60s vs 120s)
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(60_000);
        debug!(%timeout_ms, "ReadOnlyBashTool::execute: timeout_ms value");

        debug!("ReadOnlyBashTool::execute: spawning command");
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
                debug!(status = ?output.status, "ReadOnlyBashTool::execute: command completed");
                output
            }
            Ok(Err(e)) => {
                debug!(%e, "ReadOnlyBashTool::execute: failed to execute command");
                return ToolResult::error(format!("Failed to execute command: {}", e));
            }
            Err(_) => {
                debug!("ReadOnlyBashTool::execute: command timed out");
                return ToolResult::error(format!("Command timed out after {}ms", timeout_ms));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(stdout_len = %stdout.len(), stderr_len = %stderr.len(), "ReadOnlyBashTool::execute: output lengths");

        let result = if stdout.is_empty() && !stderr.is_empty() {
            debug!("ReadOnlyBashTool::execute: using stderr only");
            stderr.to_string()
        } else if stderr.is_empty() {
            debug!("ReadOnlyBashTool::execute: using stdout only");
            stdout.to_string()
        } else {
            debug!("ReadOnlyBashTool::execute: combining stdout and stderr");
            format!("{}\n\nSTDERR:\n{}", stdout, stderr)
        };

        // Truncate long output (slightly smaller limit for exploration)
        let truncated = if result.len() > 20_000 {
            debug!("ReadOnlyBashTool::execute: truncating long output");
            format!("{}...\n[truncated, {} chars total]", &result[..20_000], result.len())
        } else {
            debug!("ReadOnlyBashTool::execute: output within size limit");
            result
        };

        if output.status.success() {
            debug!("ReadOnlyBashTool::execute: command succeeded");
            ToolResult::success(truncated)
        } else {
            debug!(exit_code = ?output.status.code(), "ReadOnlyBashTool::execute: command failed");
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
    async fn test_read_only_bash_allows_read_commands() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadOnlyBashTool;

        // These should be allowed
        let result = tool.execute(serde_json::json!({"command": "echo hello"}), &ctx).await;
        assert!(!result.is_error, "echo should be allowed");

        let result = tool.execute(serde_json::json!({"command": "pwd"}), &ctx).await;
        assert!(!result.is_error, "pwd should be allowed");

        let result = tool.execute(serde_json::json!({"command": "ls -la"}), &ctx).await;
        assert!(!result.is_error, "ls should be allowed");

        let result = tool
            .execute(serde_json::json!({"command": "cat /etc/hostname"}), &ctx)
            .await;
        // cat is allowed (reading), though file might not exist
        assert!(
            !result.is_error || result.content.contains("Exit code"),
            "cat should be allowed"
        );
    }

    #[tokio::test]
    async fn test_read_only_bash_blocks_write_commands() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadOnlyBashTool;

        // File modification
        let result = tool.execute(serde_json::json!({"command": "rm file.txt"}), &ctx).await;
        assert!(result.is_error, "rm should be blocked");
        assert!(result.content.contains("blocked"));

        let result = tool.execute(serde_json::json!({"command": "mkdir newdir"}), &ctx).await;
        assert!(result.is_error, "mkdir should be blocked");

        let result = tool
            .execute(serde_json::json!({"command": "touch newfile"}), &ctx)
            .await;
        assert!(result.is_error, "touch should be blocked");

        let result = tool
            .execute(serde_json::json!({"command": "mv old.txt new.txt"}), &ctx)
            .await;
        assert!(result.is_error, "mv should be blocked");
    }

    #[tokio::test]
    async fn test_read_only_bash_blocks_redirects() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadOnlyBashTool;

        let result = tool
            .execute(serde_json::json!({"command": "echo hello > file.txt"}), &ctx)
            .await;
        assert!(result.is_error, "> redirect should be blocked");

        let result = tool
            .execute(serde_json::json!({"command": "echo hello >> file.txt"}), &ctx)
            .await;
        assert!(result.is_error, ">> redirect should be blocked");
    }

    #[tokio::test]
    async fn test_read_only_bash_blocks_git_writes() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadOnlyBashTool;

        let result = tool
            .execute(serde_json::json!({"command": "git push origin main"}), &ctx)
            .await;
        assert!(result.is_error, "git push should be blocked");

        let result = tool
            .execute(serde_json::json!({"command": "git commit -m 'test'"}), &ctx)
            .await;
        assert!(result.is_error, "git commit should be blocked");

        let result = tool
            .execute(serde_json::json!({"command": "git reset --hard"}), &ctx)
            .await;
        assert!(result.is_error, "git reset should be blocked");
    }

    #[tokio::test]
    async fn test_read_only_bash_allows_git_reads() {
        let temp = tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(temp.path())
            .output()
            .unwrap();

        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadOnlyBashTool;

        let result = tool.execute(serde_json::json!({"command": "git status"}), &ctx).await;
        assert!(!result.is_error, "git status should be allowed");

        let result = tool
            .execute(serde_json::json!({"command": "git log --oneline"}), &ctx)
            .await;
        // Even if there are no commits, the command itself should not be blocked
        assert!(
            !result.is_error || result.content.contains("Exit code"),
            "git log should be allowed"
        );

        let result = tool.execute(serde_json::json!({"command": "git diff"}), &ctx).await;
        assert!(!result.is_error, "git diff should be allowed");
    }

    #[tokio::test]
    async fn test_read_only_bash_blocks_piped_write_commands() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = ReadOnlyBashTool;

        // Commands after pipes should also be checked
        let result = tool
            .execute(serde_json::json!({"command": "ls | rm file.txt"}), &ctx)
            .await;
        assert!(result.is_error, "rm after pipe should be blocked");

        let result = tool
            .execute(serde_json::json!({"command": "cat file.txt && rm file.txt"}), &ctx)
            .await;
        assert!(result.is_error, "rm after && should be blocked");
    }

    #[test]
    fn test_is_blocked_detection() {
        // Direct commands
        assert!(ReadOnlyBashTool::is_blocked("rm file.txt").is_some());
        assert!(ReadOnlyBashTool::is_blocked("mkdir newdir").is_some());
        assert!(ReadOnlyBashTool::is_blocked("git push").is_some());

        // Commands after pipe
        assert!(ReadOnlyBashTool::is_blocked("ls | rm file").is_some());

        // Commands after &&
        assert!(ReadOnlyBashTool::is_blocked("true && rm file").is_some());

        // Redirects
        assert!(ReadOnlyBashTool::is_blocked("echo hello > file").is_some());
        assert!(ReadOnlyBashTool::is_blocked("cat foo >> bar").is_some());

        // Safe commands
        assert!(ReadOnlyBashTool::is_blocked("ls -la").is_none());
        assert!(ReadOnlyBashTool::is_blocked("cat file.txt").is_none());
        assert!(ReadOnlyBashTool::is_blocked("git status").is_none());
        assert!(ReadOnlyBashTool::is_blocked("git log").is_none());
        assert!(ReadOnlyBashTool::is_blocked("echo hello | grep h").is_none());
    }
}
