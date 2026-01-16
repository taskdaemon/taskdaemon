//! Plan domain type
//!
//! A Plan is the top-level work unit, created from user input via the Plan Refinement Loop.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use taskstore::{IndexValue, Record, now_ms};

use super::id::generate_id;
use super::priority::Priority;

/// Plan status in the workflow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    /// Being refined via Plan Refinement Loop
    #[default]
    Draft,
    /// User approved, ready for Spec decomposition
    Ready,
    /// Specs being generated/implemented
    InProgress,
    /// All Specs complete
    Complete,
    /// Unrecoverable error
    Failed,
    /// User cancelled
    Cancelled,
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Ready => write!(f, "ready"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Complete => write!(f, "complete"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A Plan is the top-level work unit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Unique identifier (e.g., "019430-plan-add-oauth")
    pub id: String,

    /// Human-readable title (max 256 chars)
    pub title: String,

    /// Current status in the workflow
    pub status: PlanStatus,

    /// Absolute path to the plan markdown file
    pub file: String,

    /// Priority for scheduler ordering
    pub priority: Priority,

    /// Creation timestamp (Unix milliseconds)
    pub created_at: i64,

    /// Last update timestamp (Unix milliseconds)
    pub updated_at: i64,
}

impl Plan {
    /// Create a new Plan with generated ID
    pub fn new(title: impl Into<String>, file: impl Into<String>) -> Self {
        let title = title.into();
        let now = now_ms();
        Self {
            id: generate_id("plan", &title),
            title,
            status: PlanStatus::Draft,
            file: file.into(),
            priority: Priority::Normal,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a Plan with a specific ID (for testing or recovery)
    pub fn with_id(id: impl Into<String>, title: impl Into<String>, file: impl Into<String>) -> Self {
        let now = now_ms();
        Self {
            id: id.into(),
            title: title.into(),
            status: PlanStatus::Draft,
            file: file.into(),
            priority: Priority::Normal,
            created_at: now,
            updated_at: now,
        }
    }

    /// Update the status
    pub fn set_status(&mut self, status: PlanStatus) {
        self.status = status;
        self.updated_at = now_ms();
    }

    /// Update the priority
    pub fn set_priority(&mut self, priority: Priority) {
        self.priority = priority;
        self.updated_at = now_ms();
    }

    /// Check if the plan is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            PlanStatus::Complete | PlanStatus::Failed | PlanStatus::Cancelled
        )
    }

    /// Check if the plan can be started
    pub fn is_startable(&self) -> bool {
        self.status == PlanStatus::Ready
    }
}

impl Record for Plan {
    fn id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn collection_name() -> &'static str {
        "plans"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        let mut fields = HashMap::new();
        fields.insert("status".to_string(), IndexValue::String(self.status.to_string()));
        fields.insert("priority".to_string(), IndexValue::String(self.priority.to_string()));
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_new() {
        let plan = Plan::new("Add OAuth Authentication", "/path/to/plan.md");
        assert!(plan.id.contains("-plan-"));
        assert!(plan.id.contains("add-oauth-authentication"));
        assert_eq!(plan.title, "Add OAuth Authentication");
        assert_eq!(plan.status, PlanStatus::Draft);
        assert_eq!(plan.priority, Priority::Normal);
    }

    #[test]
    fn test_plan_with_id() {
        let plan = Plan::with_id("test-id", "Test Plan", "/path/to/plan.md");
        assert_eq!(plan.id, "test-id");
        assert_eq!(plan.title, "Test Plan");
    }

    #[test]
    fn test_plan_set_status() {
        let mut plan = Plan::new("Test", "/test.md");
        let original_updated = plan.updated_at;

        // Sleep briefly to ensure time advances
        std::thread::sleep(std::time::Duration::from_millis(1));

        plan.set_status(PlanStatus::Ready);
        assert_eq!(plan.status, PlanStatus::Ready);
        assert!(plan.updated_at >= original_updated);
    }

    #[test]
    fn test_plan_is_terminal() {
        let mut plan = Plan::new("Test", "/test.md");
        assert!(!plan.is_terminal());

        plan.set_status(PlanStatus::Complete);
        assert!(plan.is_terminal());

        plan.set_status(PlanStatus::Failed);
        assert!(plan.is_terminal());

        plan.set_status(PlanStatus::Cancelled);
        assert!(plan.is_terminal());
    }

    #[test]
    fn test_plan_is_startable() {
        let mut plan = Plan::new("Test", "/test.md");
        assert!(!plan.is_startable()); // Draft

        plan.set_status(PlanStatus::Ready);
        assert!(plan.is_startable());

        plan.set_status(PlanStatus::InProgress);
        assert!(!plan.is_startable());
    }

    #[test]
    fn test_plan_indexed_fields() {
        let plan = Plan::new("Test", "/test.md");
        let fields = plan.indexed_fields();

        assert_eq!(fields.get("status"), Some(&IndexValue::String("draft".to_string())));
        assert_eq!(fields.get("priority"), Some(&IndexValue::String("normal".to_string())));
    }

    #[test]
    fn test_plan_serde() {
        let plan = Plan::new("Test Plan", "/path/to/plan.md");
        let json = serde_json::to_string(&plan).unwrap();
        let deserialized: Plan = serde_json::from_str(&json).unwrap();

        assert_eq!(plan.id, deserialized.id);
        assert_eq!(plan.title, deserialized.title);
        assert_eq!(plan.status, deserialized.status);
    }
}
