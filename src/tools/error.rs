//! Tool error types

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during tool execution
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Path {path} escapes worktree {worktree}")]
    SandboxViolation { path: PathBuf, worktree: PathBuf },

    #[error("File not found: {path}")]
    FileNotFound {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Must read file before editing: {path}")]
    EditWithoutRead { path: String },

    #[error("Command timed out after {timeout_ms}ms")]
    CommandTimeout { timeout_ms: u64 },

    #[error("Tool not found: {name}")]
    UnknownTool { name: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("String pattern '{pattern}' not found in file")]
    PatternNotFound { pattern: String },

    #[error("String pattern found {count} times, expected 1 (use replace_all=true for multiple)")]
    PatternNotUnique { count: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_violation_message() {
        let err = ToolError::SandboxViolation {
            path: PathBuf::from("/etc/passwd"),
            worktree: PathBuf::from("/tmp/worktree"),
        };

        let msg = err.to_string();
        assert!(msg.contains("/etc/passwd"));
        assert!(msg.contains("/tmp/worktree"));
    }

    #[test]
    fn test_pattern_not_unique_message() {
        let err = ToolError::PatternNotUnique { count: 5 };

        let msg = err.to_string();
        assert!(msg.contains("5"));
        assert!(msg.contains("replace_all"));
    }
}
