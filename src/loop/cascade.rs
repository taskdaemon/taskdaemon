//! Cascade logic for Plan → Spec → Phase pipeline
//!
//! When a loop completes, the cascade triggers the next stage:
//! - Plan (Draft → Ready) triggers Spec decomposition loop
//! - Spec decomposition completes → Specs created in TaskStore
//! - Spec phases schedule Phase loops based on dependencies

use std::sync::Arc;

use eyre::Result;
use tracing::{debug, info, warn};

use crate::domain::{LoopExecution, Plan, PlanStatus, Spec, SpecStatus};
use crate::state::StateManager;

/// Handles cascade logic between loop levels
pub struct CascadeHandler {
    state: Arc<StateManager>,
}

impl CascadeHandler {
    /// Create a new cascade handler
    pub fn new(state: Arc<StateManager>) -> Self {
        Self { state }
    }

    /// Handle completion of a Plan loop
    ///
    /// When a Plan refinement loop completes (Plan is Ready), this spawns
    /// a Spec decomposition loop to break down the Plan into Specs.
    pub async fn on_plan_complete(&self, plan: &Plan) -> Result<Option<LoopExecution>> {
        if plan.status != PlanStatus::Ready {
            debug!(plan_id = %plan.id, status = ?plan.status, "Plan not ready, skipping cascade");
            return Ok(None);
        }

        info!(plan_id = %plan.id, "Plan ready, creating Spec decomposition loop");

        // Create a Spec decomposition loop execution
        let exec = LoopExecution::new("spec", &plan.id)
            .with_context_value("plan-id", &plan.id)
            .with_context_value("plan-file", &plan.file)
            .with_context_value("plan-title", &plan.title);

        self.state.create_loop_execution(exec.clone()).await?;

        info!(exec_id = %exec.id, plan_id = %plan.id, "Created Spec decomposition loop");
        Ok(Some(exec))
    }

    /// Handle completion of a Spec decomposition loop
    ///
    /// When the Spec decomposition loop completes, update the Plan status
    /// to InProgress and check for ready Specs.
    pub async fn on_spec_decomposition_complete(&self, plan_id: &str) -> Result<Vec<LoopExecution>> {
        info!(plan_id, "Spec decomposition complete, scheduling ready Specs");

        // Update Plan status to InProgress
        if let Ok(Some(mut plan)) = self.state.get_plan(plan_id).await {
            plan.set_status(PlanStatus::InProgress);
            let _ = self.state.update_plan(plan).await;
        }

        // Find all Specs for this Plan that are ready
        self.get_ready_specs(plan_id).await
    }

    /// Get all Specs for a Plan that are ready to run (deps satisfied)
    pub async fn get_ready_specs(&self, plan_id: &str) -> Result<Vec<LoopExecution>> {
        // Get all Specs for this Plan
        let specs = self.state.list_specs_for_plan(plan_id).await?;

        // Get completed Spec IDs for dependency checking
        let completed_ids: Vec<&str> = specs
            .iter()
            .filter(|s| s.status == SpecStatus::Complete)
            .map(|s| s.id.as_str())
            .collect();

        // Find Specs that are ready (pending + deps satisfied)
        let mut ready_execs = Vec::new();
        for spec in specs.iter() {
            if spec.is_ready(&completed_ids)
                && let Some(exec) = self.create_phase_loop_for_spec(spec).await?
            {
                ready_execs.push(exec);
            }
        }

        Ok(ready_execs)
    }

    /// Create a Phase loop execution for a Spec
    async fn create_phase_loop_for_spec(&self, spec: &Spec) -> Result<Option<LoopExecution>> {
        // Check if there's already a running loop for this Spec
        let existing = self.state.get_loop_execution_for_spec(&spec.id).await?;
        if let Some(existing) = existing
            && !existing.is_terminal()
        {
            debug!(spec_id = %spec.id, exec_id = %existing.id, "Loop already running for Spec");
            return Ok(None);
        }

        // Get current phase
        let current_phase_idx = spec.current_phase_index().unwrap_or(0);
        let current_phase = spec.phases.get(current_phase_idx);

        let exec = LoopExecution::new("phase", &spec.id)
            .with_context_value("spec-id", &spec.id)
            .with_context_value("spec-file", &spec.file)
            .with_context_value("spec-title", &spec.title)
            .with_context_value("phase-number", &(current_phase_idx + 1).to_string())
            .with_context_value("total-phases", &spec.phases.len().to_string())
            .with_context_value(
                "phase-name",
                current_phase.map(|p| p.name.as_str()).unwrap_or("Unknown"),
            )
            .with_context_value(
                "phase-description",
                current_phase.map(|p| p.description.as_str()).unwrap_or(""),
            );

        self.state.create_loop_execution(exec.clone()).await?;

        info!(
            exec_id = %exec.id,
            spec_id = %spec.id,
            phase = current_phase_idx + 1,
            total = spec.phases.len(),
            "Created Phase loop for Spec"
        );

        Ok(Some(exec))
    }

    /// Handle completion of a Phase loop
    ///
    /// When a Phase loop completes, mark the phase as complete and check
    /// if there are more phases to run or if the Spec is complete.
    pub async fn on_phase_complete(&self, exec: &LoopExecution) -> Result<Option<LoopExecution>> {
        let spec_id = exec
            .context
            .get("spec-id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("Phase execution missing spec-id in context"))?;

        let phase_number: usize = exec
            .context
            .get("phase-number")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let phase_idx = phase_number - 1;

        // Update the Spec's phase status
        let mut spec = self.state.get_spec_required(spec_id).await?;
        spec.complete_phase(phase_idx);

        if spec.all_phases_complete() {
            // All phases done - mark Spec as Complete
            spec.set_status(SpecStatus::Complete);
            self.state.update_spec(spec.clone()).await?;
            info!(spec_id, "All phases complete, Spec marked Complete");

            // Check if all Specs for the Plan are complete
            self.check_plan_completion(&spec.parent).await?;

            // Wake any Specs that were blocked on this one
            return self.wake_dependent_specs(&spec.id).await;
        }

        // More phases to run - create next Phase loop
        self.state.update_spec(spec.clone()).await?;
        self.create_phase_loop_for_spec(&spec).await
    }

    /// Wake Specs that depend on the completed Spec
    async fn wake_dependent_specs(&self, completed_spec_id: &str) -> Result<Option<LoopExecution>> {
        // Get the completed Spec to find its parent Plan
        let completed_spec = self.state.get_spec_required(completed_spec_id).await?;
        let plan_id = &completed_spec.parent;

        // Get all Specs for this Plan
        let specs = self.state.list_specs_for_plan(plan_id).await?;

        // Get completed Spec IDs
        let completed_ids: Vec<&str> = specs
            .iter()
            .filter(|s| s.status == SpecStatus::Complete)
            .map(|s| s.id.as_str())
            .collect();

        // Find first Spec that is now ready
        for spec in specs.iter() {
            if spec.status == SpecStatus::Pending && spec.deps.iter().all(|dep| completed_ids.contains(&dep.as_str())) {
                info!(
                    spec_id = %spec.id,
                    depends_on = %completed_spec_id,
                    "Waking dependent Spec"
                );
                return self.create_phase_loop_for_spec(spec).await;
            }
        }

        Ok(None)
    }

    /// Check if all Specs for a Plan are complete
    async fn check_plan_completion(&self, plan_id: &str) -> Result<()> {
        let specs = self.state.list_specs_for_plan(plan_id).await?;

        // Check if all Specs are in terminal state
        let all_complete = specs.iter().all(|s| s.status == SpecStatus::Complete);
        let any_failed = specs.iter().any(|s| s.status == SpecStatus::Failed);

        if any_failed && let Ok(Some(mut plan)) = self.state.get_plan(plan_id).await {
            plan.set_status(PlanStatus::Failed);
            let _ = self.state.update_plan(plan).await;
            warn!(plan_id, "Plan marked Failed due to Spec failure");
        } else if all_complete
            && !specs.is_empty()
            && let Ok(Some(mut plan)) = self.state.get_plan(plan_id).await
        {
            plan.set_status(PlanStatus::Complete);
            let _ = self.state.update_plan(plan).await;
            info!(plan_id, "All Specs complete, Plan marked Complete");
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
    fn test_spec_ready_check_no_deps() {
        let spec = Spec::new("plan-1", "Test Spec", "/test.md");
        let completed: Vec<&str> = vec![];
        assert!(spec.is_ready(&completed));
    }

    #[test]
    fn test_spec_ready_check_with_deps_satisfied() {
        let mut spec = Spec::new("plan-1", "Test Spec", "/test.md");
        spec.add_dependency("dep-1");
        spec.add_dependency("dep-2");

        let completed: Vec<&str> = vec!["dep-1", "dep-2"];
        assert!(spec.is_ready(&completed));
    }

    #[test]
    fn test_spec_ready_check_with_deps_unsatisfied() {
        let mut spec = Spec::new("plan-1", "Test Spec", "/test.md");
        spec.add_dependency("dep-1");
        spec.add_dependency("dep-2");

        let completed: Vec<&str> = vec!["dep-1"]; // Missing dep-2
        assert!(!spec.is_ready(&completed));
    }

    #[test]
    fn test_phase_completion_index() {
        let mut spec = Spec::new("plan-1", "Test Spec", "/test.md");
        spec.add_phase(Phase::new("Phase 1", "First"));
        spec.add_phase(Phase::new("Phase 2", "Second"));
        spec.add_phase(Phase::new("Phase 3", "Third"));

        assert_eq!(spec.current_phase_index(), Some(0));
        assert!(!spec.all_phases_complete());

        spec.complete_phase(0);
        assert_eq!(spec.current_phase_index(), Some(1));

        spec.complete_phase(1);
        assert_eq!(spec.current_phase_index(), Some(2));

        spec.complete_phase(2);
        assert!(spec.all_phases_complete());
        assert_eq!(spec.current_phase_index(), None);
    }
}
