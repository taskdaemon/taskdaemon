//! LoopExecution domain type
//!
//! Tracks the runtime state of any loop (plan, spec, phase, or ralph).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use taskstore::{IndexValue, Record, now_ms};
use tracing::debug;

use super::id::generate_id;

/// Loop execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoopExecutionStatus {
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

impl std::fmt::Display for LoopExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug!(?self, "LoopExecutionStatus::fmt: called");
        match self {
            Self::Draft => {
                debug!("LoopExecutionStatus::fmt: Draft branch");
                write!(f, "draft")
            }
            Self::Pending => {
                debug!("LoopExecutionStatus::fmt: Pending branch");
                write!(f, "pending")
            }
            Self::Running => {
                debug!("LoopExecutionStatus::fmt: Running branch");
                write!(f, "running")
            }
            Self::Paused => {
                debug!("LoopExecutionStatus::fmt: Paused branch");
                write!(f, "paused")
            }
            Self::Rebasing => {
                debug!("LoopExecutionStatus::fmt: Rebasing branch");
                write!(f, "rebasing")
            }
            Self::Blocked => {
                debug!("LoopExecutionStatus::fmt: Blocked branch");
                write!(f, "blocked")
            }
            Self::Complete => {
                debug!("LoopExecutionStatus::fmt: Complete branch");
                write!(f, "complete")
            }
            Self::Failed => {
                debug!("LoopExecutionStatus::fmt: Failed branch");
                write!(f, "failed")
            }
            Self::Stopped => {
                debug!("LoopExecutionStatus::fmt: Stopped branch");
                write!(f, "stopped")
            }
        }
    }
}

/// Tracks the runtime state of a loop execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopExecution {
    /// Unique identifier
    pub id: String,

    /// Loop type name (matches a type loaded by LoopLoader)
    pub loop_type: String,

    /// Short title for display (LLM-generated from task description)
    #[serde(default)]
    pub title: Option<String>,

    /// Parent record ID (depends on loop type hierarchy)
    pub parent: Option<String>,

    /// Execution dependencies (LoopExecution IDs that must complete first)
    pub deps: Vec<String>,

    /// Current status
    pub status: LoopExecutionStatus,

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

    /// Creation timestamp (Unix milliseconds)
    pub created_at: i64,

    /// Last update timestamp (Unix milliseconds)
    pub updated_at: i64,
}

impl LoopExecution {
    /// Create a new LoopExecution with generated ID
    pub fn new(loop_type: impl Into<String>, description: impl Into<String>) -> Self {
        let loop_type = loop_type.into();
        let description = description.into();
        debug!(%loop_type, %description, "LoopExecution::new: called");
        let now = now_ms();

        Self {
            id: generate_id("loop", &format!("{}-{}", loop_type, description)),
            loop_type,
            title: None,
            parent: None,
            deps: Vec::new(),
            status: LoopExecutionStatus::Pending,
            worktree: None,
            iteration: 0,
            progress: String::new(),
            context: Value::Null,
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create with a specific ID (for testing or recovery)
    pub fn with_id(id: impl Into<String>, loop_type: impl Into<String>) -> Self {
        let id = id.into();
        let loop_type = loop_type.into();
        debug!(%id, %loop_type, "LoopExecution::with_id: called");
        let now = now_ms();
        Self {
            id,
            loop_type,
            title: None,
            parent: None,
            deps: Vec::new(),
            status: LoopExecutionStatus::Pending,
            worktree: None,
            iteration: 0,
            progress: String::new(),
            context: Value::Null,
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set the title
    pub fn set_title(&mut self, title: impl Into<String>) {
        let title = title.into();
        debug!(%self.id, %title, "LoopExecution::set_title: called");
        self.title = Some(title);
        self.updated_at = now_ms();
    }

    /// Builder method to set title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        let title = title.into();
        debug!(%self.id, %title, "LoopExecution::with_title: called");
        self.title = Some(title);
        self
    }

    /// Set the parent record
    pub fn set_parent(&mut self, parent: impl Into<String>) {
        let parent = parent.into();
        debug!(%self.id, %parent, "LoopExecution::set_parent: called");
        self.parent = Some(parent);
        self.updated_at = now_ms();
    }

    /// Set the worktree path
    pub fn set_worktree(&mut self, path: impl Into<String>) {
        let path = path.into();
        debug!(%self.id, %path, "LoopExecution::set_worktree: called");
        self.worktree = Some(path);
        self.updated_at = now_ms();
    }

    /// Set the context
    pub fn set_context(&mut self, context: Value) {
        debug!(%self.id, ?context, "LoopExecution::set_context: called");
        self.context = context;
        self.updated_at = now_ms();
    }

    /// Update the status
    pub fn set_status(&mut self, status: LoopExecutionStatus) {
        debug!(%self.id, ?status, "LoopExecution::set_status: called");
        self.status = status;
        self.updated_at = now_ms();
    }

    /// Set an error
    pub fn set_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        debug!(%self.id, %error, "LoopExecution::set_error: called");
        self.last_error = Some(error);
        self.updated_at = now_ms();
    }

    /// Clear the error
    pub fn clear_error(&mut self) {
        debug!(%self.id, "LoopExecution::clear_error: called");
        self.last_error = None;
        self.updated_at = now_ms();
    }

    /// Increment the iteration counter
    pub fn increment_iteration(&mut self) {
        debug!(%self.id, self.iteration, "LoopExecution::increment_iteration: called");
        self.iteration += 1;
        self.updated_at = now_ms();
    }

    /// Append to progress
    pub fn append_progress(&mut self, text: &str) {
        debug!(%self.id, %text, "LoopExecution::append_progress: called");
        if !self.progress.is_empty() {
            debug!("LoopExecution::append_progress: progress not empty, adding newline");
            self.progress.push('\n');
        } else {
            debug!("LoopExecution::append_progress: progress empty, no newline needed");
        }
        self.progress.push_str(text);
        self.updated_at = now_ms();
    }

    /// Check if the loop is in a terminal state
    pub fn is_terminal(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopExecution::is_terminal: called");
        let result = matches!(
            self.status,
            LoopExecutionStatus::Complete | LoopExecutionStatus::Failed | LoopExecutionStatus::Stopped
        );
        if result {
            debug!("LoopExecution::is_terminal: is terminal state");
        } else {
            debug!("LoopExecution::is_terminal: not terminal state");
        }
        result
    }

    /// Check if the loop is active (running or rebasing)
    pub fn is_active(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopExecution::is_active: called");
        let result = matches!(
            self.status,
            LoopExecutionStatus::Running | LoopExecutionStatus::Rebasing
        );
        if result {
            debug!("LoopExecution::is_active: is active");
        } else {
            debug!("LoopExecution::is_active: not active");
        }
        result
    }

    /// Check if the loop can be resumed
    pub fn is_resumable(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopExecution::is_resumable: called");
        let result = matches!(self.status, LoopExecutionStatus::Paused | LoopExecutionStatus::Blocked);
        if result {
            debug!("LoopExecution::is_resumable: is resumable");
        } else {
            debug!("LoopExecution::is_resumable: not resumable");
        }
        result
    }

    /// Check if the loop is in draft status (awaiting user approval)
    pub fn is_draft(&self) -> bool {
        debug!(%self.id, ?self.status, "LoopExecution::is_draft: called");
        let result = matches!(self.status, LoopExecutionStatus::Draft);
        if result {
            debug!("LoopExecution::is_draft: is draft");
        } else {
            debug!("LoopExecution::is_draft: not draft");
        }
        result
    }

    /// Transition from Draft to Pending (marks the draft as ready to run)
    /// The daemon will pick up pending executions and set them to Running.
    /// Returns true if the transition was made, false if not in Draft status
    pub fn mark_ready(&mut self) -> bool {
        debug!(%self.id, ?self.status, "LoopExecution::mark_ready: called");
        if self.status == LoopExecutionStatus::Draft {
            debug!("LoopExecution::mark_ready: was draft, transitioning to pending");
            self.status = LoopExecutionStatus::Pending;
            self.updated_at = now_ms();
            true
        } else {
            debug!("LoopExecution::mark_ready: not draft, no transition");
            false
        }
    }

    // === Builder methods for cascade logic ===

    /// Set the parent and return self (builder pattern)
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        let parent = parent.into();
        debug!(%self.id, %parent, "LoopExecution::with_parent: called");
        self.parent = Some(parent);
        self.updated_at = now_ms();
        self
    }

    /// Add a context value (builder pattern)
    pub fn with_context_value(mut self, key: &str, value: &str) -> Self {
        debug!(%self.id, %key, %value, "LoopExecution::with_context_value: called");
        if self.context.is_null() {
            debug!("LoopExecution::with_context_value: context is null, creating empty object");
            self.context = serde_json::json!({});
        } else {
            debug!("LoopExecution::with_context_value: context already exists");
        }
        if let Some(obj) = self.context.as_object_mut() {
            debug!("LoopExecution::with_context_value: inserting key-value pair");
            obj.insert(key.to_string(), Value::String(value.to_string()));
        } else {
            debug!("LoopExecution::with_context_value: context not an object, skipping insert");
        }
        self.updated_at = now_ms();
        self
    }
}

impl Record for LoopExecution {
    fn id(&self) -> &str {
        debug!(%self.id, "LoopExecution::id: called");
        &self.id
    }

    fn updated_at(&self) -> i64 {
        debug!(%self.id, self.updated_at, "LoopExecution::updated_at: called");
        self.updated_at
    }

    fn collection_name() -> &'static str {
        debug!("LoopExecution::collection_name: called");
        "loop_executions"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        debug!(%self.id, "LoopExecution::indexed_fields: called");
        let mut fields = HashMap::new();
        fields.insert("status".to_string(), IndexValue::String(self.status.to_string()));
        fields.insert("loop_type".to_string(), IndexValue::String(self.loop_type.clone()));
        if let Some(ref parent) = self.parent {
            debug!(%parent, "LoopExecution::indexed_fields: has parent");
            fields.insert("parent".to_string(), IndexValue::String(parent.clone()));
        } else {
            debug!("LoopExecution::indexed_fields: no parent");
        }
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_execution_new() {
        let exec = LoopExecution::new("phase", "oauth-endpoints-p1");
        assert!(exec.id.contains("-loop-"));
        assert!(exec.id.contains("phase-oauth-endpoints-p1"));
        assert_eq!(exec.loop_type, "phase");
        assert_eq!(exec.status, LoopExecutionStatus::Pending);
        assert_eq!(exec.iteration, 0);
    }

    #[test]
    fn test_loop_execution_with_parent() {
        let mut exec = LoopExecution::new("phase", "test");
        exec.set_parent("spec-123");
        assert_eq!(exec.parent, Some("spec-123".to_string()));
    }

    #[test]
    fn test_loop_execution_iteration() {
        let mut exec = LoopExecution::new("ralph", "test");
        assert_eq!(exec.iteration, 0);

        exec.increment_iteration();
        assert_eq!(exec.iteration, 1);

        exec.increment_iteration();
        assert_eq!(exec.iteration, 2);
    }

    #[test]
    fn test_loop_execution_progress() {
        let mut exec = LoopExecution::new("ralph", "test");
        assert!(exec.progress.is_empty());

        exec.append_progress("Iteration 1: Created files");
        exec.append_progress("Iteration 2: Fixed bug");

        assert!(exec.progress.contains("Iteration 1"));
        assert!(exec.progress.contains("Iteration 2"));
        assert!(exec.progress.contains('\n'));
    }

    #[test]
    fn test_loop_execution_is_terminal() {
        let mut exec = LoopExecution::new("ralph", "test");
        assert!(!exec.is_terminal());

        exec.set_status(LoopExecutionStatus::Running);
        assert!(!exec.is_terminal());

        exec.set_status(LoopExecutionStatus::Complete);
        assert!(exec.is_terminal());

        exec.set_status(LoopExecutionStatus::Failed);
        assert!(exec.is_terminal());

        exec.set_status(LoopExecutionStatus::Stopped);
        assert!(exec.is_terminal());
    }

    #[test]
    fn test_loop_execution_is_active() {
        let mut exec = LoopExecution::new("ralph", "test");

        exec.set_status(LoopExecutionStatus::Running);
        assert!(exec.is_active());

        exec.set_status(LoopExecutionStatus::Rebasing);
        assert!(exec.is_active());

        exec.set_status(LoopExecutionStatus::Paused);
        assert!(!exec.is_active());
    }

    #[test]
    fn test_loop_execution_is_resumable() {
        let mut exec = LoopExecution::new("ralph", "test");

        exec.set_status(LoopExecutionStatus::Paused);
        assert!(exec.is_resumable());

        exec.set_status(LoopExecutionStatus::Blocked);
        assert!(exec.is_resumable());

        exec.set_status(LoopExecutionStatus::Running);
        assert!(!exec.is_resumable());
    }

    #[test]
    fn test_loop_execution_error() {
        let mut exec = LoopExecution::new("ralph", "test");
        assert!(exec.last_error.is_none());

        exec.set_error("Something went wrong");
        assert_eq!(exec.last_error, Some("Something went wrong".to_string()));

        exec.clear_error();
        assert!(exec.last_error.is_none());
    }

    #[test]
    fn test_loop_execution_indexed_fields() {
        let mut exec = LoopExecution::new("phase", "test");
        exec.set_parent("spec-123");

        let fields = exec.indexed_fields();
        assert_eq!(fields.get("status"), Some(&IndexValue::String("pending".to_string())));
        assert_eq!(fields.get("loop_type"), Some(&IndexValue::String("phase".to_string())));
        assert_eq!(fields.get("parent"), Some(&IndexValue::String("spec-123".to_string())));
    }

    #[test]
    fn test_loop_execution_serde() {
        let mut exec = LoopExecution::new("ralph", "test-task");
        exec.set_context(serde_json::json!({
            "task-description": "Fix the bug",
            "working-directory": "/tmp/test"
        }));

        let json = serde_json::to_string(&exec).unwrap();
        let deserialized: LoopExecution = serde_json::from_str(&json).unwrap();

        assert_eq!(exec.id, deserialized.id);
        assert_eq!(exec.context, deserialized.context);
    }

    #[test]
    fn test_loop_execution_draft_status() {
        let mut exec = LoopExecution::new("plan", "test-plan");
        exec.set_status(LoopExecutionStatus::Draft);

        assert!(exec.is_draft());
        assert!(!exec.is_terminal());
        assert!(!exec.is_active());
        assert!(!exec.is_resumable());
    }

    #[test]
    fn test_loop_execution_mark_ready() {
        let mut exec = LoopExecution::new("plan", "test-plan");
        exec.set_status(LoopExecutionStatus::Draft);
        assert!(exec.is_draft());

        // mark_ready should transition Draft -> Pending (ready for daemon)
        let result = exec.mark_ready();
        assert!(result);
        assert_eq!(exec.status, LoopExecutionStatus::Pending);
        assert!(!exec.is_draft());

        // Calling mark_ready when not in Draft should return false
        let result = exec.mark_ready();
        assert!(!result);
        assert_eq!(exec.status, LoopExecutionStatus::Pending);
    }

    #[test]
    fn test_draft_status_serialization() {
        let mut exec = LoopExecution::new("plan", "test-plan");
        exec.set_status(LoopExecutionStatus::Draft);

        let json = serde_json::to_string(&exec).unwrap();
        assert!(json.contains("\"status\":\"draft\""));

        let deserialized: LoopExecution = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status, LoopExecutionStatus::Draft);
    }

    #[test]
    fn test_draft_status_display() {
        assert_eq!(LoopExecutionStatus::Draft.to_string(), "draft");
    }
}
