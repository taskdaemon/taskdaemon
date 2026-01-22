//! LoopRun domain type
//!
//! Tracks the runtime state of any loop (plan, spec, phase, or ralph).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use taskstore::{IndexValue, Record, now_ms};
use tracing::debug;

use super::id::generate_id;

/// Loop run status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoopRunStatus {
    /// Plan created, awaiting user approval
    Draft,
    /// Waiting to start
    #[default]
    Pending,
    /// Actively iterating
    Running,
    /// User paused
    Paused,
    /// Handling main branch update
    Rebasing,
    /// Rebase conflict or other blocker
    Blocked,
    /// Validation passed
    Complete,
    /// Max iterations or unrecoverable error
    Failed,
    /// User/coordinator requested stop
    Stopped,
}

impl std::fmt::Display for LoopRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug!(?self, "LoopRunStatus::fmt: called");
        match self {
            Self::Draft => {
                debug!("LoopRunStatus::fmt: Draft branch");
                write!(f, "draft")
            }
            Self::Pending => {
                debug!("LoopRunStatus::fmt: Pending branch");
                write!(f, "pending")
            }
            Self::Running => {
                debug!("LoopRunStatus::fmt: Running branch");
                write!(f, "running")
            }
            Self::Paused => {
                debug!("LoopRunStatus::fmt: Paused branch");
                write!(f, "paused")
            }
            Self::Rebasing => {
                debug!("LoopRunStatus::fmt: Rebasing branch");
                write!(f, "rebasing")
            }
            Self::Blocked => {
                debug!("LoopRunStatus::fmt: Blocked branch");
                write!(f, "blocked")
            }
            Self::Complete => {
                debug!("LoopRunStatus::fmt: Complete branch");
                write!(f, "complete")
            }
            Self::Failed => {
                debug!("LoopRunStatus::fmt: Failed branch");
                write!(f, "failed")
            }
            Self::Stopped => {
                debug!("LoopRunStatus::fmt: Stopped branch");
                write!(f, "stopped")
            }
        }
    }
}

/// Tracks the runtime state of a loop run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRun {
    /// Unique identifier
    pub id: String,

    /// Loop type name (matches a type loaded by LoopLoader)
    pub loop_type: String,

    /// Short title for display (LLM-generated from task description)
    #[serde(default)]
    pub title: Option<String>,

    /// Parent record ID (depends on loop type hierarchy)
    pub parent: Option<String>,

    /// Run dependencies (LoopRun IDs that must complete first)
    pub deps: Vec<String>,

    /// Current status
    pub status: LoopRunStatus,

    /// Absolute path to git worktree (None for plan/spec loops)
    pub worktree: Option<String>,

    /// Current iteration (1-indexed)
    pub iteration: u32,

    /// Accumulated progress text from previous iterations
    pub progress: String,

    /// Template context (JSON) for prompt rendering
    pub context: Value,

    /// Last error message (if any)
    pub last_error: Option<String>,

    /// Path to primary artifact (e.g., ".taskdaemon/plans/{id}/plan.md")
    #[serde(default)]
    pub artifact_path: Option<String>,

    /// Artifact validation status: "draft" | "complete" | "failed"
    #[serde(default)]
    pub artifact_status: Option<String>,

    /// Total LLM input tokens consumed across all iterations
    #[serde(default)]
    pub total_input_tokens: u64,

    /// Total LLM output tokens generated across all iterations
    #[serde(default)]
    pub total_output_tokens: u64,

    /// Total validation execution time in milliseconds
    #[serde(default)]
    pub total_duration_ms: u64,

    /// Creation timestamp (Unix milliseconds)
    pub created_at: i64,

    /// Last update timestamp (Unix milliseconds)
    pub updated_at: i64,
}

impl LoopRun {
    /// Create a new LoopRun with generated ID
    pub fn new(loop_type: impl Into<String>, description: impl Into<String>) -> Self {
        let loop_type = loop_type.into();
        let description = description.into();
        debug!(%loop_type, %description, "LoopRun::new: called");
        let now = now_ms();

        Self {
            id: generate_id("loop", &format!("{}-{}", loop_type, description)),
            loop_type,
            title: None,
            parent: None,
            deps: Vec::new(),
            status: LoopRunStatus::Pending,
            worktree: None,
            iteration: 0,
            progress: String::new(),
            context: Value::Null,
            last_error: None,
            artifact_path: None,
            artifact_status: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_duration_ms: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create with a specific ID (for testing or recovery)
    pub fn with_id(id: impl Into<String>, loop_type: impl Into<String>) -> Self {
        let id = id.into();
        let loop_type = loop_type.into();
        debug!(%id, %loop_type, "LoopRun::with_id: called");
        let now = now_ms();
        Self {
            id,
            loop_type,
            title: None,
            parent: None,
            deps: Vec::new(),
            status: LoopRunStatus::Pending,
            worktree: None,
            iteration: 0,
            progress: String::new(),
            context: Value::Null,
            last_error: None,
            artifact_path: None,
            artifact_status: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_duration_ms: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set the title
    pub fn set_title(&mut self, title: impl Into<String>) {
        let title = title.into();
        debug!(%self.id, %title, "LoopRun::set_title: called");
        self.title = Some(title);
        self.updated_at = now_ms();
    }

    /// Builder method to set title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        let title = title.into();
        debug!(%self.id, %title, "LoopRun::with_title: called");
        self.title = Some(title);
        self
    }

    /// Set the artifact path and mark status as draft
    pub fn set_artifact(&mut self, path: impl Into<String>) {
        let path = path.into();
        debug!(%self.id, %path, "LoopRun::set_artifact: called");
        self.artifact_path = Some(path);
        self.artifact_status = Some("draft".to_string());
        self.updated_at = now_ms();
    }

    /// Builder method to set artifact path and status
    pub fn with_artifact(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        debug!(%self.id, %path, "LoopRun::with_artifact: called");
        self.artifact_path = Some(path);
        self.artifact_status = Some("draft".to_string());
        self
    }

    /// Update artifact status
    pub fn set_artifact_status(&mut self, status: impl Into<String>) {
        let status = status.into();
        debug!(%self.id, %status, "LoopRun::set_artifact_status: called");
        self.artifact_status = Some(status);
        self.updated_at = now_ms();
    }

    /// Add tokens and duration from a completed iteration
    pub fn add_iteration_metrics(&mut self, input_tokens: u64, output_tokens: u64, duration_ms: u64) {
        debug!(
            %self.id,
            input_tokens,
            output_tokens,
            duration_ms,
            "LoopRun::add_iteration_metrics: called"
        );
        self.total_input_tokens += input_tokens;
        self.total_output_tokens += output_tokens;
        self.total_duration_ms += duration_ms;
        self.updated_at = now_ms();
    }

    /// Get total tokens consumed
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }

    /// Set the parent record
    pub fn set_parent(&mut self, parent: impl Into<String>) {
        let parent = parent.into();
        debug!(%self.id, %parent, "LoopRun::set_parent: called");
        self.parent = Some(parent);
        self.updated_at = now_ms();
    }

    /// Set the worktree path
    pub fn set_worktree(&mut self, path: impl Into<String>) {
        let path = path.into();
        debug!(%self.id, %path, "LoopRun::set_worktree: called");
        self.worktree = Some(path);
        self.updated_at = now_ms();
    }

    /// Set the context
    pub fn set_context(&mut self, context: Value) {
        debug!(%self.id, ?context, "LoopRun::set_context: called");
        self.context = context;
        self.updated_at = now_ms();
    }

    /// Update the status
    pub fn set_status(&mut self, status: LoopRunStatus) {
        debug!(%self.id, ?status, "LoopRun::set_status: called");
        self.status = status;
        self.updated_at = now_ms();
    }

    /// Set an error
    pub fn set_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        debug!(%self.id, %error, "LoopRun::set_error: called");
        self.last_error = Some(error);
        self.updated_at = now_ms();
    }

    /// Clear the error
    pub fn clear_error(&mut self) {
        debug!(%self.id, "LoopRun::clear_error: called");
        self.last_error = None;
        self.updated_at = now_ms();
    }

    /// Increment the iteration counter
    pub fn increment_iteration(&mut self) {
        debug!(%self.id, self.iteration, "LoopRun::increment_iteration: called");
        self.iteration += 1;
        self.updated_at = now_ms();
    }

    /// Append to progress
    pub fn append_progress(&mut self, text: &str) {
        debug!(%self.id, %text, "LoopRun::append_progress: called");
        if !self.progress.is_empty() {
            debug!("LoopRun::append_progress: progress not empty, adding newline");
            self.progress.push('\n');
        } else {
            debug!("LoopRun::append_progress: progress empty, no newline needed");
        }
        self.progress.push_str(text);
        self.updated_at = now_ms();
    }

    /// Check if the loop is in a terminal state
    pub fn is_terminal(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopRun::is_terminal: called");
        let result = matches!(
            self.status,
            LoopRunStatus::Complete | LoopRunStatus::Failed | LoopRunStatus::Stopped
        );
        if result {
            debug!("LoopRun::is_terminal: is terminal state");
        } else {
            debug!("LoopRun::is_terminal: not terminal state");
        }
        result
    }

    /// Check if the loop is active (running or rebasing)
    pub fn is_active(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopRun::is_active: called");
        let result = matches!(self.status, LoopRunStatus::Running | LoopRunStatus::Rebasing);
        if result {
            debug!("LoopRun::is_active: is active");
        } else {
            debug!("LoopRun::is_active: not active");
        }
        result
    }

    /// Check if the loop can be resumed
    pub fn is_resumable(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopRun::is_resumable: called");
        let result = matches!(self.status, LoopRunStatus::Paused | LoopRunStatus::Blocked);
        if result {
            debug!("LoopRun::is_resumable: is resumable");
        } else {
            debug!("LoopRun::is_resumable: not resumable");
        }
        result
    }

    /// Check if the loop is in draft status (awaiting user approval)
    pub fn is_draft(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopRun::is_draft: called");
        let result = matches!(self.status, LoopRunStatus::Draft);
        if result {
            debug!("LoopRun::is_draft: is draft");
        } else {
            debug!("LoopRun::is_draft: not draft");
        }
        result
    }

    /// Transition from Draft to Pending (marks the draft as ready to run)
    /// The daemon will pick up pending runs and set them to Running.
    /// Returns true if the transition was made, false if not in Draft status
    pub fn mark_ready(&mut self) -> bool {
        debug!(%self.id, ?self.status, "LoopRun::mark_ready: called");
        if self.status == LoopRunStatus::Draft {
            debug!("LoopRun::mark_ready: was draft, transitioning to pending");
            self.status = LoopRunStatus::Pending;
            self.updated_at = now_ms();
            true
        } else {
            debug!("LoopRun::mark_ready: not draft, no transition");
            false
        }
    }

    // === Builder methods for cascade logic ===

    /// Set the parent and return self (builder pattern)
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        let parent = parent.into();
        debug!(%self.id, %parent, "LoopRun::with_parent: called");
        self.parent = Some(parent);
        self.updated_at = now_ms();
        self
    }

    /// Add a context value (builder pattern)
    pub fn with_context_value(mut self, key: &str, value: &str) -> Self {
        debug!(%self.id, %key, %value, "LoopRun::with_context_value: called");
        if self.context.is_null() {
            debug!("LoopRun::with_context_value: context is null, creating empty object");
            self.context = serde_json::json!({});
        } else {
            debug!("LoopRun::with_context_value: context already exists");
        }
        if let Some(obj) = self.context.as_object_mut() {
            debug!("LoopRun::with_context_value: inserting key-value pair");
            obj.insert(key.to_string(), Value::String(value.to_string()));
        } else {
            debug!("LoopRun::with_context_value: context not an object, skipping insert");
        }
        self.updated_at = now_ms();
        self
    }
}

impl Record for LoopRun {
    fn id(&self) -> &str {
        debug!(%self.id, "LoopRun::id: called");
        &self.id
    }

    fn updated_at(&self) -> i64 {
        debug!(%self.id, self.updated_at, "LoopRun::updated_at: called");
        self.updated_at
    }

    fn collection_name() -> &'static str {
        debug!("LoopRun::collection_name: called");
        // Keep collection name for backward compatibility with existing data
        "loop_executions"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        debug!(%self.id, "LoopRun::indexed_fields: called");
        let mut fields = HashMap::new();
        fields.insert("status".to_string(), IndexValue::String(self.status.to_string()));
        fields.insert("loop_type".to_string(), IndexValue::String(self.loop_type.clone()));
        if let Some(ref parent) = self.parent {
            debug!(%parent, "LoopRun::indexed_fields: has parent");
            fields.insert("parent".to_string(), IndexValue::String(parent.clone()));
        } else {
            debug!("LoopRun::indexed_fields: no parent");
        }
        fields
    }
}

// Type aliases for backward compatibility
pub type LoopExecution = LoopRun;
pub type LoopExecutionStatus = LoopRunStatus;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_run_new() {
        let run = LoopRun::new("phase", "oauth-endpoints-p1");
        assert!(run.id.contains("-loop-"));
        assert!(run.id.contains("phase-oauth-endpoints-p1"));
        assert_eq!(run.loop_type, "phase");
        assert_eq!(run.status, LoopRunStatus::Pending);
        assert_eq!(run.iteration, 0);
    }

    #[test]
    fn test_loop_run_with_parent() {
        let mut run = LoopRun::new("phase", "test");
        run.set_parent("spec-123");
        assert_eq!(run.parent, Some("spec-123".to_string()));
    }

    #[test]
    fn test_loop_run_iteration() {
        let mut run = LoopRun::new("ralph", "test");
        assert_eq!(run.iteration, 0);

        run.increment_iteration();
        assert_eq!(run.iteration, 1);

        run.increment_iteration();
        assert_eq!(run.iteration, 2);
    }

    #[test]
    fn test_loop_run_progress() {
        let mut run = LoopRun::new("ralph", "test");
        assert!(run.progress.is_empty());

        run.append_progress("Iteration 1: Created files");
        run.append_progress("Iteration 2: Fixed bug");

        assert!(run.progress.contains("Iteration 1"));
        assert!(run.progress.contains("Iteration 2"));
        assert!(run.progress.contains('\n'));
    }

    #[test]
    fn test_loop_run_is_terminal() {
        let mut run = LoopRun::new("ralph", "test");
        assert!(!run.is_terminal());

        run.set_status(LoopRunStatus::Running);
        assert!(!run.is_terminal());

        run.set_status(LoopRunStatus::Complete);
        assert!(run.is_terminal());

        run.set_status(LoopRunStatus::Failed);
        assert!(run.is_terminal());

        run.set_status(LoopRunStatus::Stopped);
        assert!(run.is_terminal());
    }

    #[test]
    fn test_loop_run_is_active() {
        let mut run = LoopRun::new("ralph", "test");

        run.set_status(LoopRunStatus::Running);
        assert!(run.is_active());

        run.set_status(LoopRunStatus::Rebasing);
        assert!(run.is_active());

        run.set_status(LoopRunStatus::Paused);
        assert!(!run.is_active());
    }

    #[test]
    fn test_loop_run_is_resumable() {
        let mut run = LoopRun::new("ralph", "test");

        run.set_status(LoopRunStatus::Paused);
        assert!(run.is_resumable());

        run.set_status(LoopRunStatus::Blocked);
        assert!(run.is_resumable());

        run.set_status(LoopRunStatus::Running);
        assert!(!run.is_resumable());
    }

    #[test]
    fn test_loop_run_error() {
        let mut run = LoopRun::new("ralph", "test");
        assert!(run.last_error.is_none());

        run.set_error("Something went wrong");
        assert_eq!(run.last_error, Some("Something went wrong".to_string()));

        run.clear_error();
        assert!(run.last_error.is_none());
    }

    #[test]
    fn test_loop_run_indexed_fields() {
        let mut run = LoopRun::new("phase", "test");
        run.set_parent("spec-123");

        let fields = run.indexed_fields();
        assert_eq!(fields.get("status"), Some(&IndexValue::String("pending".to_string())));
        assert_eq!(fields.get("loop_type"), Some(&IndexValue::String("phase".to_string())));
        assert_eq!(fields.get("parent"), Some(&IndexValue::String("spec-123".to_string())));
    }

    #[test]
    fn test_loop_run_serde() {
        let mut run = LoopRun::new("ralph", "test-task");
        run.set_context(serde_json::json!({
            "task-description": "Fix the bug",
            "working-directory": "/tmp/test"
        }));

        let json = serde_json::to_string(&run).unwrap();
        let deserialized: LoopRun = serde_json::from_str(&json).unwrap();

        assert_eq!(run.id, deserialized.id);
        assert_eq!(run.context, deserialized.context);
    }

    #[test]
    fn test_loop_run_draft_status() {
        let mut run = LoopRun::new("plan", "test-plan");
        run.set_status(LoopRunStatus::Draft);

        assert!(run.is_draft());
        assert!(!run.is_terminal());
        assert!(!run.is_active());
        assert!(!run.is_resumable());
    }

    #[test]
    fn test_loop_run_mark_ready() {
        let mut run = LoopRun::new("plan", "test-plan");
        run.set_status(LoopRunStatus::Draft);
        assert!(run.is_draft());

        // mark_ready should transition Draft -> Pending (ready for daemon)
        let result = run.mark_ready();
        assert!(result);
        assert_eq!(run.status, LoopRunStatus::Pending);
        assert!(!run.is_draft());

        // Calling mark_ready when not in Draft should return false
        let result = run.mark_ready();
        assert!(!result);
        assert_eq!(run.status, LoopRunStatus::Pending);
    }

    #[test]
    fn test_draft_status_serialization() {
        let mut run = LoopRun::new("plan", "test-plan");
        run.set_status(LoopRunStatus::Draft);

        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains("\"status\":\"draft\""));

        let deserialized: LoopRun = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status, LoopRunStatus::Draft);
    }

    #[test]
    fn test_draft_status_display() {
        assert_eq!(LoopRunStatus::Draft.to_string(), "draft");
    }

    // Test backward compatibility aliases
    #[test]
    fn test_type_alias_compatibility() {
        let exec: LoopExecution = LoopRun::new("phase", "test");
        assert_eq!(exec.status, LoopExecutionStatus::Pending);
    }
}
