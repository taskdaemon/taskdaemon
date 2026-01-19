//! Generic Loop type
//!
//! Loop is the unified record type for all loop types.
//! The `r#type` field determines the record's behavior based on
//! definitions loaded from YAML configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use taskstore::{IndexValue, Record, now_ms};
use tracing::debug;

use super::id::generate_id;
use super::priority::Priority;

/// Generic status that can be used for any loop type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    /// Initial state, not yet started
    #[default]
    Pending,
    /// Being worked on
    Running,
    /// Waiting for dependencies
    Blocked,
    /// Ready for next stage
    Ready,
    /// Work in progress (sub-records being processed)
    InProgress,
    /// Successfully completed
    Complete,
    /// Failed with error
    Failed,
    /// Cancelled by user
    Cancelled,
}

impl std::fmt::Display for LoopStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug!(?self, "LoopStatus::fmt: called");
        match self {
            Self::Pending => {
                debug!("LoopStatus::fmt: Pending branch");
                write!(f, "pending")
            }
            Self::Running => {
                debug!("LoopStatus::fmt: Running branch");
                write!(f, "running")
            }
            Self::Blocked => {
                debug!("LoopStatus::fmt: Blocked branch");
                write!(f, "blocked")
            }
            Self::Ready => {
                debug!("LoopStatus::fmt: Ready branch");
                write!(f, "ready")
            }
            Self::InProgress => {
                debug!("LoopStatus::fmt: InProgress branch");
                write!(f, "in_progress")
            }
            Self::Complete => {
                debug!("LoopStatus::fmt: Complete branch");
                write!(f, "complete")
            }
            Self::Failed => {
                debug!("LoopStatus::fmt: Failed branch");
                write!(f, "failed")
            }
            Self::Cancelled => {
                debug!("LoopStatus::fmt: Cancelled branch");
                write!(f, "cancelled")
            }
        }
    }
}

/// Phase status within a record
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    #[default]
    Pending,
    Running,
    Complete,
    Failed,
}

/// A Phase is a unit of work within a record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    /// Phase name
    pub name: String,

    /// Phase description
    pub description: String,

    /// Current status
    pub status: PhaseStatus,
}

impl Phase {
    /// Create a new Phase
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let name = name.into();
        let description = description.into();
        debug!(%name, %description, "Phase::new: called");
        Self {
            name,
            description,
            status: PhaseStatus::Pending,
        }
    }

    /// Check if the phase is complete
    pub fn is_complete(&self) -> bool {
        debug!(%self.name, ?self.status, "Phase::is_complete: called");
        let result = self.status == PhaseStatus::Complete;
        if result {
            debug!("Phase::is_complete: is complete");
        } else {
            debug!("Phase::is_complete: not complete");
        }
        result
    }
}

/// A generic loop record that can represent any loop type's work unit.
/// The r#type field determines behavior and relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Loop {
    /// Unique identifier
    pub id: String,

    /// Loop type name (matches a type loaded from YAML configuration)
    #[serde(rename = "type")]
    pub r#type: String,

    /// Human-readable title
    pub title: String,

    /// Current status in the workflow
    pub status: LoopStatus,

    /// Parent record ID (based on loop type hierarchy)
    pub parent: Option<String>,

    /// Record IDs that must complete before this can start
    pub deps: Vec<String>,

    /// Absolute path to the markdown file (if any)
    pub file: Option<String>,

    /// Phases to implement (for types that use phases)
    pub phases: Vec<Phase>,

    /// Priority for scheduler ordering
    pub priority: Priority,

    /// Additional type-specific context data
    pub context: serde_json::Value,

    /// Creation timestamp (Unix milliseconds)
    pub created_at: i64,

    /// Last update timestamp (Unix milliseconds)
    pub updated_at: i64,
}

impl Loop {
    /// Create a new Loop with generated ID
    pub fn new(r#type: impl Into<String>, title: impl Into<String>) -> Self {
        let r#type = r#type.into();
        let title = title.into();
        debug!(%r#type, %title, "Loop::new: called");
        let now = now_ms();
        Self {
            id: generate_id(&r#type, &title),
            r#type,
            title,
            status: LoopStatus::Pending,
            parent: None,
            deps: Vec::new(),
            file: None,
            phases: Vec::new(),
            priority: Priority::Normal,
            context: serde_json::Value::Null,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a Loop with a specific ID (for testing or recovery)
    pub fn with_id(id: impl Into<String>, r#type: impl Into<String>, title: impl Into<String>) -> Self {
        let id = id.into();
        let r#type = r#type.into();
        let title = title.into();
        debug!(%id, %r#type, %title, "Loop::with_id: called");
        let now = now_ms();
        Self {
            id,
            r#type,
            title,
            status: LoopStatus::Pending,
            parent: None,
            deps: Vec::new(),
            file: None,
            phases: Vec::new(),
            priority: Priority::Normal,
            context: serde_json::Value::Null,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set the parent record
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        let parent = parent.into();
        debug!(%self.id, %parent, "Loop::with_parent: called");
        self.parent = Some(parent);
        self.updated_at = now_ms();
        self
    }

    /// Set the file path
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        let file = file.into();
        debug!(%self.id, %file, "Loop::with_file: called");
        self.file = Some(file);
        self.updated_at = now_ms();
        self
    }

    /// Add a dependency
    pub fn add_dependency(&mut self, dep_id: impl Into<String>) {
        let dep_id = dep_id.into();
        debug!(%self.id, %dep_id, "Loop::add_dependency: called");
        self.deps.push(dep_id);
        self.updated_at = now_ms();
    }

    /// Add a phase
    pub fn add_phase(&mut self, phase: Phase) {
        debug!(%self.id, %phase.name, "Loop::add_phase: called");
        self.phases.push(phase);
        self.updated_at = now_ms();
    }

    /// Update the status
    pub fn set_status(&mut self, status: LoopStatus) {
        debug!(%self.id, ?status, "Loop::set_status: called");
        self.status = status;
        self.updated_at = now_ms();
    }

    /// Update the priority
    pub fn set_priority(&mut self, priority: Priority) {
        debug!(%self.id, ?priority, "Loop::set_priority: called");
        self.priority = priority;
        self.updated_at = now_ms();
    }

    /// Set context data
    pub fn set_context(&mut self, context: serde_json::Value) {
        debug!(%self.id, ?context, "Loop::set_context: called");
        self.context = context;
        self.updated_at = now_ms();
    }

    /// Check if the record is ready to run (all deps complete)
    pub fn is_ready(&self, completed_records: &[&str]) -> bool {
        debug!(%self.id, ?self.status, ?self.deps, "Loop::is_ready: called");
        let is_pending = self.status == LoopStatus::Pending;
        let deps_complete = self.deps.iter().all(|dep| completed_records.contains(&dep.as_str()));
        let result = is_pending && deps_complete;
        if result {
            debug!("Loop::is_ready: is ready");
        } else if !is_pending {
            debug!("Loop::is_ready: not pending");
        } else {
            debug!("Loop::is_ready: deps not complete");
        }
        result
    }

    /// Check if all phases are complete
    pub fn all_phases_complete(&self) -> bool {
        debug!(%self.id, num_phases = self.phases.len(), "Loop::all_phases_complete: called");
        let result = !self.phases.is_empty() && self.phases.iter().all(|p| p.is_complete());
        if result {
            debug!("Loop::all_phases_complete: all phases complete");
        } else if self.phases.is_empty() {
            debug!("Loop::all_phases_complete: no phases");
        } else {
            debug!("Loop::all_phases_complete: some phases incomplete");
        }
        result
    }

    /// Get the current phase (first non-complete phase)
    pub fn current_phase(&self) -> Option<&Phase> {
        debug!(%self.id, "Loop::current_phase: called");
        let result = self.phases.iter().find(|p| p.status != PhaseStatus::Complete);
        if let Some(phase) = &result {
            debug!(%phase.name, "Loop::current_phase: found");
        } else {
            debug!("Loop::current_phase: no current phase");
        }
        result
    }

    /// Get the current phase index (0-indexed)
    pub fn current_phase_index(&self) -> Option<usize> {
        debug!(%self.id, "Loop::current_phase_index: called");
        let result = self.phases.iter().position(|p| p.status != PhaseStatus::Complete);
        if let Some(idx) = result {
            debug!(idx, "Loop::current_phase_index: found");
        } else {
            debug!("Loop::current_phase_index: no current phase");
        }
        result
    }

    /// Mark a phase as complete by index
    pub fn complete_phase(&mut self, index: usize) {
        debug!(%self.id, index, "Loop::complete_phase: called");
        if let Some(phase) = self.phases.get_mut(index) {
            debug!(%phase.name, "Loop::complete_phase: marking complete");
            phase.status = PhaseStatus::Complete;
            self.updated_at = now_ms();
        } else {
            debug!("Loop::complete_phase: index out of bounds");
        }
    }

    /// Check if the record is in a terminal state
    pub fn is_terminal(&self) -> bool {
        debug!(%self.id, ?self.status, "Loop::is_terminal: called");
        let result = matches!(
            self.status,
            LoopStatus::Complete | LoopStatus::Failed | LoopStatus::Cancelled
        );
        if result {
            debug!("Loop::is_terminal: is terminal");
        } else {
            debug!("Loop::is_terminal: not terminal");
        }
        result
    }

    /// Check if the record can be started
    pub fn is_startable(&self) -> bool {
        debug!(%self.id, ?self.status, "Loop::is_startable: called");
        let result = self.status == LoopStatus::Ready;
        if result {
            debug!("Loop::is_startable: is startable");
        } else {
            debug!("Loop::is_startable: not startable");
        }
        result
    }
}

impl Record for Loop {
    fn id(&self) -> &str {
        debug!(%self.id, "Loop::id: called");
        &self.id
    }

    fn updated_at(&self) -> i64 {
        debug!(%self.id, self.updated_at, "Loop::updated_at: called");
        self.updated_at
    }

    fn collection_name() -> &'static str {
        debug!("Loop::collection_name: called");
        "records"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        debug!(%self.id, "Loop::indexed_fields: called");
        let mut fields = HashMap::new();
        fields.insert("type".to_string(), IndexValue::String(self.r#type.clone()));
        fields.insert("status".to_string(), IndexValue::String(self.status.to_string()));
        fields.insert("priority".to_string(), IndexValue::String(self.priority.to_string()));
        if let Some(parent) = &self.parent {
            debug!(%parent, "Loop::indexed_fields: has parent");
            fields.insert("parent".to_string(), IndexValue::String(parent.clone()));
        } else {
            debug!("Loop::indexed_fields: no parent");
        }
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_record_new() {
        let record = Loop::new("mytype", "Do Something");
        assert!(record.id.contains("-mytype-"));
        assert_eq!(record.r#type, "mytype");
        assert_eq!(record.title, "Do Something");
        assert_eq!(record.status, LoopStatus::Pending);
        assert!(record.parent.is_none());
        assert!(record.deps.is_empty());
    }

    #[test]
    fn test_loop_record_with_parent() {
        let record = Loop::new("child", "Child Task").with_parent("parent-id");
        assert_eq!(record.parent, Some("parent-id".to_string()));
    }

    #[test]
    fn test_loop_record_with_file() {
        let record = Loop::new("doc", "Document").with_file("/path/to/doc.md");
        assert_eq!(record.file, Some("/path/to/doc.md".to_string()));
    }

    #[test]
    fn test_loop_record_phases() {
        let mut record = Loop::new("impl", "Implementation");
        record.add_phase(Phase::new("Phase 1", "Setup"));
        record.add_phase(Phase::new("Phase 2", "Implement"));

        assert_eq!(record.phases.len(), 2);
        assert_eq!(record.current_phase_index(), Some(0));
        assert!(!record.all_phases_complete());

        record.complete_phase(0);
        assert_eq!(record.current_phase_index(), Some(1));

        record.complete_phase(1);
        assert!(record.all_phases_complete());
    }

    #[test]
    fn test_loop_record_is_ready() {
        let mut record = Loop::new("task", "Task");
        record.add_dependency("dep-1");
        record.add_dependency("dep-2");

        // Not ready - missing deps
        assert!(!record.is_ready(&["dep-1"]));

        // Ready - all deps complete
        assert!(record.is_ready(&["dep-1", "dep-2"]));

        // Not ready if not Pending
        record.set_status(LoopStatus::Running);
        assert!(!record.is_ready(&["dep-1", "dep-2"]));
    }

    #[test]
    fn test_loop_record_is_terminal() {
        let mut record = Loop::new("task", "Task");
        assert!(!record.is_terminal());

        record.set_status(LoopStatus::Complete);
        assert!(record.is_terminal());

        record.set_status(LoopStatus::Failed);
        assert!(record.is_terminal());

        record.set_status(LoopStatus::Cancelled);
        assert!(record.is_terminal());
    }

    #[test]
    fn test_loop_record_indexed_fields() {
        let record = Loop::new("mytype", "Test").with_parent("parent-id");
        let fields = record.indexed_fields();

        assert_eq!(fields.get("type"), Some(&IndexValue::String("mytype".to_string())));
        assert_eq!(fields.get("status"), Some(&IndexValue::String("pending".to_string())));
        assert_eq!(fields.get("parent"), Some(&IndexValue::String("parent-id".to_string())));
    }

    #[test]
    fn test_loop_record_serde() {
        let mut record = Loop::new("task", "Test Task")
            .with_parent("parent-id")
            .with_file("/test.md");
        record.add_phase(Phase::new("Phase 1", "Description"));
        record.add_dependency("dep-1");

        let json = serde_json::to_string(&record).unwrap();
        let deserialized: Loop = serde_json::from_str(&json).unwrap();

        assert_eq!(record.id, deserialized.id);
        assert_eq!(record.r#type, deserialized.r#type);
        assert_eq!(record.parent, deserialized.parent);
        assert_eq!(record.phases.len(), deserialized.phases.len());
        assert_eq!(record.deps, deserialized.deps);
    }
}
