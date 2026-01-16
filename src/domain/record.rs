//! Generic Loop type
//!
//! Loop is the unified record type for all loop types.
//! The `r#type` field determines the record's behavior based on
//! definitions loaded from YAML configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use taskstore::{IndexValue, Record, now_ms};

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
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Blocked => write!(f, "blocked"),
            Self::Ready => write!(f, "ready"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Complete => write!(f, "complete"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
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
        Self {
            name: name.into(),
            description: description.into(),
            status: PhaseStatus::Pending,
        }
    }

    /// Check if the phase is complete
    pub fn is_complete(&self) -> bool {
        self.status == PhaseStatus::Complete
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
        let now = now_ms();
        Self {
            id: id.into(),
            r#type: r#type.into(),
            title: title.into(),
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
        self.parent = Some(parent.into());
        self.updated_at = now_ms();
        self
    }

    /// Set the file path
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self.updated_at = now_ms();
        self
    }

    /// Add a dependency
    pub fn add_dependency(&mut self, dep_id: impl Into<String>) {
        self.deps.push(dep_id.into());
        self.updated_at = now_ms();
    }

    /// Add a phase
    pub fn add_phase(&mut self, phase: Phase) {
        self.phases.push(phase);
        self.updated_at = now_ms();
    }

    /// Update the status
    pub fn set_status(&mut self, status: LoopStatus) {
        self.status = status;
        self.updated_at = now_ms();
    }

    /// Update the priority
    pub fn set_priority(&mut self, priority: Priority) {
        self.priority = priority;
        self.updated_at = now_ms();
    }

    /// Set context data
    pub fn set_context(&mut self, context: serde_json::Value) {
        self.context = context;
        self.updated_at = now_ms();
    }

    /// Check if the record is ready to run (all deps complete)
    pub fn is_ready(&self, completed_records: &[&str]) -> bool {
        self.status == LoopStatus::Pending && self.deps.iter().all(|dep| completed_records.contains(&dep.as_str()))
    }

    /// Check if all phases are complete
    pub fn all_phases_complete(&self) -> bool {
        !self.phases.is_empty() && self.phases.iter().all(|p| p.is_complete())
    }

    /// Get the current phase (first non-complete phase)
    pub fn current_phase(&self) -> Option<&Phase> {
        self.phases.iter().find(|p| p.status != PhaseStatus::Complete)
    }

    /// Get the current phase index (0-indexed)
    pub fn current_phase_index(&self) -> Option<usize> {
        self.phases.iter().position(|p| p.status != PhaseStatus::Complete)
    }

    /// Mark a phase as complete by index
    pub fn complete_phase(&mut self, index: usize) {
        if let Some(phase) = self.phases.get_mut(index) {
            phase.status = PhaseStatus::Complete;
            self.updated_at = now_ms();
        }
    }

    /// Check if the record is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            LoopStatus::Complete | LoopStatus::Failed | LoopStatus::Cancelled
        )
    }

    /// Check if the record can be started
    pub fn is_startable(&self) -> bool {
        self.status == LoopStatus::Ready
    }
}

impl Record for Loop {
    fn id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn collection_name() -> &'static str {
        "records"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        let mut fields = HashMap::new();
        fields.insert("type".to_string(), IndexValue::String(self.r#type.clone()));
        fields.insert("status".to_string(), IndexValue::String(self.status.to_string()));
        fields.insert("priority".to_string(), IndexValue::String(self.priority.to_string()));
        if let Some(parent) = &self.parent {
            fields.insert("parent".to_string(), IndexValue::String(parent.clone()));
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
