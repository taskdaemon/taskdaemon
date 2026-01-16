//! Cascade logic for loop type hierarchy
//!
//! When a loop completes, the cascade triggers child loops based on
//! the parent-child relationships defined in loop type configs.
//! Child types declare their parent via the `parent` field in YAML.

use std::sync::{Arc, RwLock};

use eyre::Result;
use tracing::{debug, info, warn};

use crate::domain::{Loop, LoopExecution, LoopStatus};
use crate::state::StateManager;

use super::type_loader::LoopLoader;

/// Handles cascade logic between loop levels
pub struct CascadeHandler {
    state: Arc<StateManager>,
    type_loader: Arc<RwLock<LoopLoader>>,
}

impl CascadeHandler {
    /// Create a new cascade handler
    pub fn new(state: Arc<StateManager>, type_loader: Arc<RwLock<LoopLoader>>) -> Self {
        Self { state, type_loader }
    }

    /// Get child loop types for a given parent type
    fn get_child_types(&self, parent_type: &str) -> Vec<String> {
        let loader = self.type_loader.read().unwrap();
        loader
            .children_of(parent_type)
            .into_iter()
            .map(|s: &str| s.to_string())
            .collect()
    }

    /// Handle completion of a Loop record
    ///
    /// When a Loop becomes Ready, spawn child loops as defined by the type hierarchy.
    pub async fn on_loop_ready(&self, record: &Loop) -> Result<Vec<LoopExecution>> {
        if record.status != LoopStatus::Ready {
            debug!(id = %record.id, status = ?record.status, "Loop not ready, skipping cascade");
            return Ok(vec![]);
        }

        // Find child loop types for this loop's type
        let child_types = self.get_child_types(&record.r#type);
        if child_types.is_empty() {
            debug!(id = %record.id, loop_type = %record.r#type, "No child loop types defined");
            return Ok(vec![]);
        }

        info!(id = %record.id, child_types = ?child_types, "Loop ready, creating child loops");

        let mut executions = Vec::new();
        for child_type in child_types {
            let exec = LoopExecution::new(&child_type, &record.id)
                .with_context_value("parent-id", &record.id)
                .with_context_value("parent-type", &record.r#type)
                .with_context_value("parent-title", &record.title);

            if let Some(file) = &record.file {
                let exec = exec.with_context_value("parent-file", file);
                self.state.create_loop_execution(exec.clone()).await?;
                executions.push(exec);
            } else {
                self.state.create_loop_execution(exec.clone()).await?;
                executions.push(exec);
            }

            info!(exec_id = %executions.last().unwrap().id, parent_id = %record.id, child_type = %child_type, "Created child loop");
        }

        Ok(executions)
    }

    /// Handle completion of decomposition (creates children)
    ///
    /// When decomposition completes, update the parent Loop status to InProgress
    /// and find ready child Loops.
    pub async fn on_decomposition_complete(&self, parent_id: &str) -> Result<Vec<LoopExecution>> {
        info!(parent_id, "Decomposition complete, scheduling ready children");

        // Update parent Loop status to InProgress
        if let Ok(Some(mut parent)) = self.state.get_loop(parent_id).await {
            parent.set_status(LoopStatus::InProgress);
            let _ = self.state.update_loop(parent).await;
        }

        // Find all child Loops that are ready
        self.get_ready_children(parent_id).await
    }

    /// Get all child Loops for a parent that are ready to run (deps satisfied)
    pub async fn get_ready_children(&self, parent_id: &str) -> Result<Vec<LoopExecution>> {
        // Get all child Loops for this parent
        let children = self.state.list_loops_for_parent(parent_id).await?;

        // Get completed child IDs for dependency checking
        let completed_ids: Vec<&str> = children
            .iter()
            .filter(|c| c.status == LoopStatus::Complete)
            .map(|c| c.id.as_str())
            .collect();

        // Find children that are ready (pending + deps satisfied)
        let mut ready_execs = Vec::new();
        for child in children.iter() {
            if child.is_ready(&completed_ids) {
                let child_execs = self.create_child_loops_for_record(child).await?;
                ready_execs.extend(child_execs);
            }
        }

        Ok(ready_execs)
    }

    /// Create child loop executions for a Loop record based on type hierarchy
    async fn create_child_loops_for_record(&self, record: &Loop) -> Result<Vec<LoopExecution>> {
        // Check if there's already a running execution for this record
        let existing = self.state.get_loop_execution_for_spec(&record.id).await?;
        if let Some(existing) = existing
            && !existing.is_terminal()
        {
            debug!(record_id = %record.id, exec_id = %existing.id, "Execution already running for record");
            return Ok(vec![]);
        }

        // Find child loop types for this record's type
        let child_types = self.get_child_types(&record.r#type);
        if child_types.is_empty() {
            debug!(record_id = %record.id, loop_type = %record.r#type, "No child loop types defined");
            return Ok(vec![]);
        }

        // Get current phase if record has phases
        let current_phase_idx = record.current_phase_index().unwrap_or(0);
        let current_phase = record.phases.get(current_phase_idx);

        let mut executions = Vec::new();
        for child_type in child_types {
            let mut exec = LoopExecution::new(&child_type, &record.id)
                .with_context_value("record-id", &record.id)
                .with_context_value("record-type", &record.r#type)
                .with_context_value("record-title", &record.title);

            if let Some(file) = &record.file {
                exec = exec.with_context_value("record-file", file);
            }

            // Add phase context if phases exist
            if !record.phases.is_empty() {
                exec = exec
                    .with_context_value("phase-number", &(current_phase_idx + 1).to_string())
                    .with_context_value("total-phases", &record.phases.len().to_string());

                if let Some(phase) = current_phase {
                    exec = exec
                        .with_context_value("phase-name", &phase.name)
                        .with_context_value("phase-description", &phase.description);
                }
            }

            self.state.create_loop_execution(exec.clone()).await?;

            info!(
                exec_id = %exec.id,
                record_id = %record.id,
                child_type = %child_type,
                phase = current_phase_idx + 1,
                total = record.phases.len(),
                "Created child loop for record"
            );
            executions.push(exec);
        }

        Ok(executions)
    }

    /// Handle completion of a child loop execution
    ///
    /// When a child loop completes, mark the phase as complete (if applicable)
    /// and check if there are more phases to run or if the record is complete.
    pub async fn on_child_loop_complete(&self, exec: &LoopExecution) -> Result<Vec<LoopExecution>> {
        let record_id = exec
            .context
            .get("record-id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("Child execution missing record-id in context"))?;

        let phase_number: usize = exec
            .context
            .get("phase-number")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let phase_idx = phase_number - 1;

        // Update the record's phase status
        let mut record = self.state.get_loop_required(record_id).await?;

        // Only complete phase if the record has phases
        if !record.phases.is_empty() {
            record.complete_phase(phase_idx);
        }

        if record.all_phases_complete() || record.phases.is_empty() {
            // All phases done (or no phases) - mark record as Complete
            record.set_status(LoopStatus::Complete);
            self.state.update_loop(record.clone()).await?;
            info!(record_id, "All phases complete, record marked Complete");

            // Check if all children for the parent are complete
            if let Some(parent_id) = &record.parent {
                self.check_parent_completion(parent_id).await?;
            }

            // Wake any records that were blocked on this one
            return self.wake_dependent_records(&record.id).await;
        }

        // More phases to run - create next child loops
        self.state.update_loop(record.clone()).await?;
        self.create_child_loops_for_record(&record).await
    }

    /// Wake records that depend on the completed record
    async fn wake_dependent_records(&self, completed_record_id: &str) -> Result<Vec<LoopExecution>> {
        // Get the completed record to find its parent
        let completed_record = self.state.get_loop_required(completed_record_id).await?;
        let Some(parent_id) = &completed_record.parent else {
            return Ok(vec![]);
        };

        // Get all siblings (children of the same parent)
        let siblings = self.state.list_loops_for_parent(parent_id).await?;

        // Get completed sibling IDs
        let completed_ids: Vec<&str> = siblings
            .iter()
            .filter(|s| s.status == LoopStatus::Complete)
            .map(|s| s.id.as_str())
            .collect();

        // Find first sibling that is now ready
        for sibling in siblings.iter() {
            if sibling.status == LoopStatus::Pending
                && sibling.deps.iter().all(|dep| completed_ids.contains(&dep.as_str()))
            {
                info!(
                    record_id = %sibling.id,
                    depends_on = %completed_record_id,
                    "Waking dependent record"
                );
                return self.create_child_loops_for_record(sibling).await;
            }
        }

        Ok(vec![])
    }

    /// Check if all children for a parent are complete
    async fn check_parent_completion(&self, parent_id: &str) -> Result<()> {
        let children = self.state.list_loops_for_parent(parent_id).await?;

        // Check if all children are in terminal state
        let all_complete = children.iter().all(|c| c.status == LoopStatus::Complete);
        let any_failed = children.iter().any(|c| c.status == LoopStatus::Failed);

        if any_failed && let Ok(Some(mut parent)) = self.state.get_loop(parent_id).await {
            parent.set_status(LoopStatus::Failed);
            let _ = self.state.update_loop(parent).await;
            warn!(parent_id, "Parent marked Failed due to child failure");
        } else if all_complete
            && !children.is_empty()
            && let Ok(Some(mut parent)) = self.state.get_loop(parent_id).await
        {
            parent.set_status(LoopStatus::Complete);
            let _ = self.state.update_loop(parent).await;
            info!(parent_id, "All children complete, parent marked Complete");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Phase;

    // Note: Integration tests would require a mock StateManager
    // These are placeholder tests for the logic patterns

    #[test]
    fn test_loop_ready_check_no_deps() {
        let record = Loop::new("mytype", "Test Record");
        let completed: Vec<&str> = vec![];
        assert!(record.is_ready(&completed));
    }

    #[test]
    fn test_loop_ready_check_with_deps_satisfied() {
        let mut record = Loop::new("mytype", "Test Record");
        record.add_dependency("dep-1");
        record.add_dependency("dep-2");

        let completed: Vec<&str> = vec!["dep-1", "dep-2"];
        assert!(record.is_ready(&completed));
    }

    #[test]
    fn test_loop_ready_check_with_deps_unsatisfied() {
        let mut record = Loop::new("mytype", "Test Record");
        record.add_dependency("dep-1");
        record.add_dependency("dep-2");

        let completed: Vec<&str> = vec!["dep-1"]; // Missing dep-2
        assert!(!record.is_ready(&completed));
    }

    #[test]
    fn test_phase_completion_index() {
        let mut record = Loop::new("mytype", "Test Record");
        record.add_phase(Phase::new("Phase 1", "First"));
        record.add_phase(Phase::new("Phase 2", "Second"));
        record.add_phase(Phase::new("Phase 3", "Third"));

        assert_eq!(record.current_phase_index(), Some(0));
        assert!(!record.all_phases_complete());

        record.complete_phase(0);
        assert_eq!(record.current_phase_index(), Some(1));

        record.complete_phase(1);
        assert_eq!(record.current_phase_index(), Some(2));

        record.complete_phase(2);
        assert!(record.all_phases_complete());
        assert_eq!(record.current_phase_index(), None);
    }
}
