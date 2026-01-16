//! Spec domain type
//!
//! A Spec is an atomic unit of work decomposed from a Plan.
//! Contains phases that are implemented sequentially.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use taskstore::{IndexValue, Record, now_ms};

use super::id::generate_id;
use super::priority::Priority;

/// Spec status in the workflow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpecStatus {
    /// Waiting for dependencies to complete
    #[default]
    Pending,
    /// Dependency failed or manual intervention needed
    Blocked,
    /// Being implemented
    Running,
    /// All phases done, validation passed
    Complete,
    /// Max iterations or unrecoverable error
    Failed,
}

impl std::fmt::Display for SpecStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Blocked => write!(f, "blocked"),
            Self::Running => write!(f, "running"),
            Self::Complete => write!(f, "complete"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Phase status within a Spec
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    #[default]
    Pending,
    Running,
    Complete,
    Failed,
}

/// A Phase is a unit of work within a Spec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    /// Phase name (e.g., "Phase 1: Create endpoint stubs")
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

/// A Spec is an atomic unit of work decomposed from a Plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    /// Unique identifier (e.g., "019431-spec-oauth-endpoints")
    pub id: String,

    /// Parent Plan ID
    pub parent: String,

    /// Human-readable title
    pub title: String,

    /// Current status in the workflow
    pub status: SpecStatus,

    /// Spec IDs that must complete before this can start
    pub deps: Vec<String>,

    /// Absolute path to the spec markdown file
    pub file: String,

    /// Phases to implement (run sequentially)
    pub phases: Vec<Phase>,

    /// Priority for scheduler ordering
    pub priority: Priority,

    /// Creation timestamp (Unix milliseconds)
    pub created_at: i64,

    /// Last update timestamp (Unix milliseconds)
    pub updated_at: i64,
}

impl Spec {
    /// Create a new Spec with generated ID
    pub fn new(parent: impl Into<String>, title: impl Into<String>, file: impl Into<String>) -> Self {
        let title = title.into();
        let now = now_ms();
        Self {
            id: generate_id("spec", &title),
            parent: parent.into(),
            title,
            status: SpecStatus::Pending,
            deps: Vec::new(),
            file: file.into(),
            phases: Vec::new(),
            priority: Priority::Normal,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a Spec with a specific ID (for testing or recovery)
    pub fn with_id(
        id: impl Into<String>,
        parent: impl Into<String>,
        title: impl Into<String>,
        file: impl Into<String>,
    ) -> Self {
        let now = now_ms();
        Self {
            id: id.into(),
            parent: parent.into(),
            title: title.into(),
            status: SpecStatus::Pending,
            deps: Vec::new(),
            file: file.into(),
            phases: Vec::new(),
            priority: Priority::Normal,
            created_at: now,
            updated_at: now,
        }
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
    pub fn set_status(&mut self, status: SpecStatus) {
        self.status = status;
        self.updated_at = now_ms();
    }

    /// Check if the spec is ready to run (all deps complete)
    pub fn is_ready(&self, completed_specs: &[&str]) -> bool {
        self.status == SpecStatus::Pending && self.deps.iter().all(|dep| completed_specs.contains(&dep.as_str()))
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

    /// Check if the spec is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self.status, SpecStatus::Complete | SpecStatus::Failed)
    }
}

impl Record for Spec {
    fn id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn collection_name() -> &'static str {
        "specs"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        let mut fields = HashMap::new();
        fields.insert("status".to_string(), IndexValue::String(self.status.to_string()));
        fields.insert("parent".to_string(), IndexValue::String(self.parent.clone()));
        fields.insert("priority".to_string(), IndexValue::String(self.priority.to_string()));
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_new() {
        let spec = Spec::new("parent-plan-id", "OAuth Endpoints", "/path/to/spec.md");
        assert!(spec.id.contains("-spec-"));
        assert!(spec.id.contains("oauth-endpoints"));
        assert_eq!(spec.parent, "parent-plan-id");
        assert_eq!(spec.title, "OAuth Endpoints");
        assert_eq!(spec.status, SpecStatus::Pending);
        assert!(spec.deps.is_empty());
        assert!(spec.phases.is_empty());
    }

    #[test]
    fn test_spec_with_phases() {
        let mut spec = Spec::new("parent", "Test Spec", "/test.md");
        spec.add_phase(Phase::new("Phase 1", "Create stubs"));
        spec.add_phase(Phase::new("Phase 2", "Implement logic"));

        assert_eq!(spec.phases.len(), 2);
        assert_eq!(spec.current_phase_index(), Some(0));
    }

    #[test]
    fn test_spec_phase_completion() {
        let mut spec = Spec::new("parent", "Test Spec", "/test.md");
        spec.add_phase(Phase::new("Phase 1", "First"));
        spec.add_phase(Phase::new("Phase 2", "Second"));

        assert!(!spec.all_phases_complete());

        spec.complete_phase(0);
        assert_eq!(spec.current_phase_index(), Some(1));

        spec.complete_phase(1);
        assert!(spec.all_phases_complete());
    }

    #[test]
    fn test_spec_is_ready() {
        let mut spec = Spec::new("parent", "Test Spec", "/test.md");
        spec.add_dependency("dep-1");
        spec.add_dependency("dep-2");

        // Not ready - missing deps
        assert!(!spec.is_ready(&["dep-1"]));

        // Ready - all deps complete
        assert!(spec.is_ready(&["dep-1", "dep-2"]));

        // Not ready if not Pending
        spec.set_status(SpecStatus::Running);
        assert!(!spec.is_ready(&["dep-1", "dep-2"]));
    }

    #[test]
    fn test_spec_no_deps_is_ready() {
        let spec = Spec::new("parent", "Test Spec", "/test.md");
        assert!(spec.is_ready(&[]));
    }

    #[test]
    fn test_spec_indexed_fields() {
        let spec = Spec::new("parent-id", "Test", "/test.md");
        let fields = spec.indexed_fields();

        assert_eq!(fields.get("status"), Some(&IndexValue::String("pending".to_string())));
        assert_eq!(fields.get("parent"), Some(&IndexValue::String("parent-id".to_string())));
    }

    #[test]
    fn test_spec_serde() {
        let mut spec = Spec::new("parent", "Test Spec", "/test.md");
        spec.add_phase(Phase::new("Phase 1", "Description"));
        spec.add_dependency("dep-1");

        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: Spec = serde_json::from_str(&json).unwrap();

        assert_eq!(spec.id, deserialized.id);
        assert_eq!(spec.phases.len(), deserialized.phases.len());
        assert_eq!(spec.deps, deserialized.deps);
    }

    #[test]
    fn test_phase_new() {
        let phase = Phase::new("Phase 1", "Create endpoint stubs");
        assert_eq!(phase.name, "Phase 1");
        assert_eq!(phase.description, "Create endpoint stubs");
        assert_eq!(phase.status, PhaseStatus::Pending);
    }
}
