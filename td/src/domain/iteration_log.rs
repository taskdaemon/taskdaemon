//! IterationLog domain type
//!
//! Persistent record of a single loop iteration's execution.
//! Stores full validation output, tool call history, and metrics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use taskstore::{IndexValue, Record, now_ms};
use tracing::debug;

/// Summary of a tool call made during an agentic loop iteration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallSummary {
    /// Tool name (e.g., "Edit", "Bash", "Read")
    pub tool_name: String,
    /// First N chars of arguments for display
    pub arguments_summary: String,
    /// First N chars of result for display
    pub result_summary: String,
    /// Whether tool call resulted in error
    pub is_error: bool,
}

impl ToolCallSummary {
    /// Maximum length for argument and result summaries
    pub const SUMMARY_LEN: usize = 200;

    /// Create a new tool call summary, truncating arguments and result
    pub fn new(tool_name: impl Into<String>, arguments: &str, result: &str, is_error: bool) -> Self {
        let tool_name = tool_name.into();
        debug!(%tool_name, is_error, "ToolCallSummary::new: called");
        Self {
            tool_name,
            arguments_summary: arguments.chars().take(Self::SUMMARY_LEN).collect(),
            result_summary: result.chars().take(Self::SUMMARY_LEN).collect(),
            is_error,
        }
    }
}

/// Persistent record of a single loop iteration's execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationLog {
    /// Unique ID: {execution_id}-iter-{N}
    pub id: String,

    /// Parent LoopExecution ID (indexed for queries)
    pub execution_id: String,

    /// Iteration number (1-indexed for display)
    pub iteration: u32,

    /// Validation command that was run (e.g., "otto ci")
    pub validation_command: String,

    /// Exit code from validation (0 = success, -1 = error before execution)
    pub exit_code: i32,

    /// Full validation stdout (NO truncation for storage)
    pub stdout: String,

    /// Full validation stderr (NO truncation for storage)
    pub stderr: String,

    /// Validation duration in milliseconds
    pub duration_ms: u64,

    /// Files changed during this iteration (from git status)
    pub files_changed: Vec<String>,

    /// LLM input tokens consumed in this iteration
    pub llm_input_tokens: Option<u64>,

    /// LLM output tokens generated in this iteration
    pub llm_output_tokens: Option<u64>,

    /// Summary of tool calls made during agentic loop
    pub tool_calls: Vec<ToolCallSummary>,

    /// Creation timestamp (milliseconds since Unix epoch)
    pub created_at: i64,

    /// Last update timestamp
    pub updated_at: i64,
}

impl IterationLog {
    /// Create a new IterationLog
    pub fn new(execution_id: impl Into<String>, iteration: u32) -> Self {
        let execution_id = execution_id.into();
        debug!(%execution_id, iteration, "IterationLog::new: called");
        let now = now_ms();
        let id = format!("{}-iter-{}", execution_id, iteration);
        Self {
            id,
            execution_id,
            iteration,
            validation_command: String::new(),
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            files_changed: Vec::new(),
            llm_input_tokens: None,
            llm_output_tokens: None,
            tool_calls: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Builder: set validation command
    pub fn with_validation_command(mut self, command: impl Into<String>) -> Self {
        self.validation_command = command.into();
        debug!(%self.id, %self.validation_command, "IterationLog::with_validation_command");
        self
    }

    /// Builder: set exit code
    pub fn with_exit_code(mut self, exit_code: i32) -> Self {
        self.exit_code = exit_code;
        debug!(%self.id, exit_code, "IterationLog::with_exit_code");
        self
    }

    /// Builder: set stdout
    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = stdout.into();
        debug!(%self.id, stdout_len = self.stdout.len(), "IterationLog::with_stdout");
        self
    }

    /// Builder: set stderr
    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = stderr.into();
        debug!(%self.id, stderr_len = self.stderr.len(), "IterationLog::with_stderr");
        self
    }

    /// Builder: set duration
    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms;
        debug!(%self.id, duration_ms, "IterationLog::with_duration_ms");
        self
    }

    /// Builder: set files changed
    pub fn with_files_changed(mut self, files: Vec<String>) -> Self {
        debug!(%self.id, num_files = files.len(), "IterationLog::with_files_changed");
        self.files_changed = files;
        self
    }

    /// Builder: set LLM token usage
    pub fn with_llm_tokens(mut self, input: Option<u64>, output: Option<u64>) -> Self {
        debug!(%self.id, ?input, ?output, "IterationLog::with_llm_tokens");
        self.llm_input_tokens = input;
        self.llm_output_tokens = output;
        self
    }

    /// Builder: set tool calls
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCallSummary>) -> Self {
        debug!(%self.id, num_tool_calls = tool_calls.len(), "IterationLog::with_tool_calls");
        self.tool_calls = tool_calls;
        self
    }

    /// Check if this iteration succeeded (exit_code == 0)
    pub fn is_success(&self) -> bool {
        debug!(%self.id, self.exit_code, "IterationLog::is_success: called");
        self.exit_code == 0
    }

    /// Check if this iteration failed to even run (exit_code == -1)
    pub fn is_error(&self) -> bool {
        debug!(%self.id, self.exit_code, "IterationLog::is_error: called");
        self.exit_code == -1
    }

    /// Get total tokens consumed
    pub fn total_tokens(&self) -> Option<u64> {
        match (self.llm_input_tokens, self.llm_output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            (Some(input), None) => Some(input),
            (None, Some(output)) => Some(output),
            (None, None) => None,
        }
    }
}

impl Record for IterationLog {
    fn id(&self) -> &str {
        debug!(%self.id, "IterationLog::id: called");
        &self.id
    }

    fn updated_at(&self) -> i64 {
        debug!(%self.id, self.updated_at, "IterationLog::updated_at: called");
        self.updated_at
    }

    fn collection_name() -> &'static str {
        debug!("IterationLog::collection_name: called");
        "iteration_logs"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        debug!(%self.id, "IterationLog::indexed_fields: called");
        let mut fields = HashMap::new();
        fields.insert(
            "execution_id".to_string(),
            IndexValue::String(self.execution_id.clone()),
        );
        fields.insert("iteration".to_string(), IndexValue::Int(self.iteration as i64));
        fields.insert("exit_code".to_string(), IndexValue::Int(self.exit_code as i64));
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iteration_log_new() {
        let log = IterationLog::new("exec-123", 1);
        assert_eq!(log.id, "exec-123-iter-1");
        assert_eq!(log.execution_id, "exec-123");
        assert_eq!(log.iteration, 1);
        assert_eq!(log.exit_code, 0);
        assert!(log.stdout.is_empty());
        assert!(log.stderr.is_empty());
    }

    #[test]
    fn test_iteration_log_builder() {
        let log = IterationLog::new("exec-456", 2)
            .with_validation_command("otto ci")
            .with_exit_code(1)
            .with_stdout("Build failed")
            .with_stderr("Error: compilation error")
            .with_duration_ms(5000)
            .with_files_changed(vec!["src/main.rs".to_string()])
            .with_llm_tokens(Some(1000), Some(500));

        assert_eq!(log.validation_command, "otto ci");
        assert_eq!(log.exit_code, 1);
        assert_eq!(log.stdout, "Build failed");
        assert_eq!(log.stderr, "Error: compilation error");
        assert_eq!(log.duration_ms, 5000);
        assert_eq!(log.files_changed, vec!["src/main.rs"]);
        assert_eq!(log.llm_input_tokens, Some(1000));
        assert_eq!(log.llm_output_tokens, Some(500));
    }

    #[test]
    fn test_iteration_log_is_success() {
        let mut log = IterationLog::new("exec-123", 1);
        assert!(log.is_success());

        log.exit_code = 1;
        assert!(!log.is_success());
    }

    #[test]
    fn test_iteration_log_is_error() {
        let mut log = IterationLog::new("exec-123", 1);
        assert!(!log.is_error());

        log.exit_code = -1;
        assert!(log.is_error());
    }

    #[test]
    fn test_iteration_log_total_tokens() {
        let log = IterationLog::new("exec-123", 1).with_llm_tokens(Some(1000), Some(500));
        assert_eq!(log.total_tokens(), Some(1500));

        let log2 = IterationLog::new("exec-123", 1).with_llm_tokens(Some(1000), None);
        assert_eq!(log2.total_tokens(), Some(1000));

        let log3 = IterationLog::new("exec-123", 1).with_llm_tokens(None, Some(500));
        assert_eq!(log3.total_tokens(), Some(500));

        let log4 = IterationLog::new("exec-123", 1);
        assert_eq!(log4.total_tokens(), None);
    }

    #[test]
    fn test_iteration_log_indexed_fields() {
        let log = IterationLog::new("exec-789", 3).with_exit_code(1);
        let fields = log.indexed_fields();

        assert_eq!(
            fields.get("execution_id"),
            Some(&IndexValue::String("exec-789".to_string()))
        );
        assert_eq!(fields.get("iteration"), Some(&IndexValue::Int(3)));
        assert_eq!(fields.get("exit_code"), Some(&IndexValue::Int(1)));
    }

    #[test]
    fn test_iteration_log_serde() {
        let log = IterationLog::new("exec-123", 1)
            .with_validation_command("otto ci")
            .with_exit_code(0)
            .with_stdout("All tests passed")
            .with_tool_calls(vec![ToolCallSummary::new("Edit", "file.rs", "OK", false)]);

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: IterationLog = serde_json::from_str(&json).unwrap();

        assert_eq!(log.id, deserialized.id);
        assert_eq!(log.execution_id, deserialized.execution_id);
        assert_eq!(log.validation_command, deserialized.validation_command);
        assert_eq!(log.tool_calls.len(), deserialized.tool_calls.len());
    }

    #[test]
    fn test_tool_call_summary_truncation() {
        let long_args = "a".repeat(500);
        let long_result = "b".repeat(500);

        let summary = ToolCallSummary::new("TestTool", &long_args, &long_result, false);

        assert_eq!(summary.arguments_summary.len(), ToolCallSummary::SUMMARY_LEN);
        assert_eq!(summary.result_summary.len(), ToolCallSummary::SUMMARY_LEN);
    }

    #[test]
    fn test_tool_call_summary_normal() {
        let summary = ToolCallSummary::new("Edit", "src/main.rs", "File edited", false);

        assert_eq!(summary.tool_name, "Edit");
        assert_eq!(summary.arguments_summary, "src/main.rs");
        assert_eq!(summary.result_summary, "File edited");
        assert!(!summary.is_error);
    }
}
