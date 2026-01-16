//! Crash recovery
//!
//! Detects incomplete loops from TaskStore and prepares them for restart.

use tracing::{debug, info, warn};

use crate::domain::{LoopExecution, LoopExecutionStatus, Plan, PlanStatus, Spec, SpecStatus};

use super::StateManager;

/// Recovery statistics
#[derive(Debug, Default)]
pub struct RecoveryStats {
    /// Number of plans that need recovery
    pub plans_to_recover: usize,
    /// Number of specs that need recovery
    pub specs_to_recover: usize,
    /// Number of executions that need recovery
    pub executions_to_recover: usize,
}

impl std::fmt::Display for RecoveryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "plans: {}, specs: {}, executions: {}",
            self.plans_to_recover, self.specs_to_recover, self.executions_to_recover
        )
    }
}

/// Check for incomplete work and gather recovery statistics
///
/// This scans the TaskStore for:
/// - Plans in InProgress state (crashed mid-execution)
/// - Specs in Running state (crashed mid-execution)
/// - LoopExecutions in Running/Rebasing state (crashed mid-iteration)
pub async fn scan_for_recovery(state: &StateManager) -> eyre::Result<RecoveryStats> {
    let mut stats = RecoveryStats::default();

    // Find plans that were in progress
    let in_progress_plans = state
        .list_plans(Some("in_progress".to_string()))
        .await
        .map_err(|e| eyre::eyre!("Failed to list in-progress plans: {}", e))?;
    stats.plans_to_recover = in_progress_plans.len();

    for plan in &in_progress_plans {
        debug!(plan_id = %plan.id, "Found in-progress plan needing recovery");
    }

    // Find specs that were running
    let running_specs = state
        .list_specs(None, Some("running".to_string()))
        .await
        .map_err(|e| eyre::eyre!("Failed to list running specs: {}", e))?;
    stats.specs_to_recover = running_specs.len();

    for spec in &running_specs {
        debug!(spec_id = %spec.id, "Found running spec needing recovery");
    }

    // Find executions that were active (running or rebasing)
    let running_execs = state
        .list_executions(Some("running".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list running executions: {}", e))?;

    let rebasing_execs = state
        .list_executions(Some("rebasing".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list rebasing executions: {}", e))?;

    stats.executions_to_recover = running_execs.len() + rebasing_execs.len();

    for exec in running_execs.iter().chain(rebasing_execs.iter()) {
        debug!(
            exec_id = %exec.id,
            loop_type = %exec.loop_type,
            iteration = exec.iteration,
            "Found active execution needing recovery"
        );
    }

    if stats.plans_to_recover > 0 || stats.specs_to_recover > 0 || stats.executions_to_recover > 0 {
        info!("Recovery scan found incomplete work: {}", stats);
    } else {
        debug!("Recovery scan found no incomplete work");
    }

    Ok(stats)
}

/// Get all incomplete items that need recovery
pub async fn get_incomplete_items(state: &StateManager) -> eyre::Result<(Vec<Plan>, Vec<Spec>, Vec<LoopExecution>)> {
    // Get in-progress plans
    let plans = state
        .list_plans(Some("in_progress".to_string()))
        .await
        .map_err(|e| eyre::eyre!("Failed to list in-progress plans: {}", e))?;

    // Get running specs
    let specs = state
        .list_specs(None, Some("running".to_string()))
        .await
        .map_err(|e| eyre::eyre!("Failed to list running specs: {}", e))?;

    // Get active executions
    let mut executions = state
        .list_executions(Some("running".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list running executions: {}", e))?;

    let rebasing_execs = state
        .list_executions(Some("rebasing".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list rebasing executions: {}", e))?;

    executions.extend(rebasing_execs);

    Ok((plans, specs, executions))
}

/// Mark crashed executions as paused so they can be resumed
///
/// This transitions active executions to Paused state to indicate they
/// need to be resumed (rather than restarted from scratch).
pub async fn mark_crashed_as_paused(state: &StateManager) -> eyre::Result<usize> {
    let (plans, specs, executions) = get_incomplete_items(state).await?;
    let mut count = 0;

    // Mark plans as ready (so they can be picked up again)
    for mut plan in plans {
        warn!(plan_id = %plan.id, "Marking crashed plan as ready for retry");
        plan.set_status(PlanStatus::Ready);
        state
            .update_plan(plan)
            .await
            .map_err(|e| eyre::eyre!("Failed to update plan: {}", e))?;
        count += 1;
    }

    // Mark specs as pending (so they can be picked up again)
    for mut spec in specs {
        warn!(spec_id = %spec.id, "Marking crashed spec as pending for retry");
        spec.set_status(SpecStatus::Pending);
        state
            .update_spec(spec)
            .await
            .map_err(|e| eyre::eyre!("Failed to update spec: {}", e))?;
        count += 1;
    }

    // Mark executions as paused
    for mut exec in executions {
        warn!(
            exec_id = %exec.id,
            loop_type = %exec.loop_type,
            iteration = exec.iteration,
            "Marking crashed execution as paused"
        );
        exec.set_status(LoopExecutionStatus::Paused);
        exec.set_error("Recovered from crash - execution was interrupted");
        state
            .update_execution(exec)
            .await
            .map_err(|e| eyre::eyre!("Failed to update execution: {}", e))?;
        count += 1;
    }

    if count > 0 {
        info!("Marked {} crashed items as paused for recovery", count);
    }

    Ok(count)
}

/// Full recovery process: scan, mark as paused, sync store
pub async fn recover(state: &StateManager) -> eyre::Result<RecoveryStats> {
    info!("Starting crash recovery process");

    // First sync from JSONL files to ensure we have latest state
    state
        .sync()
        .await
        .map_err(|e| eyre::eyre!("Failed to sync store: {}", e))?;

    // Scan for incomplete work
    let stats = scan_for_recovery(state).await?;

    // Mark crashed items as paused
    if stats.plans_to_recover > 0 || stats.specs_to_recover > 0 || stats.executions_to_recover > 0 {
        mark_crashed_as_paused(state).await?;
    }

    info!("Crash recovery complete: {}", stats);
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_recovery_empty_store() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        let stats = scan_for_recovery(&manager).await.unwrap();

        assert_eq!(stats.plans_to_recover, 0);
        assert_eq!(stats.specs_to_recover, 0);
        assert_eq!(stats.executions_to_recover, 0);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_recovery_finds_in_progress_plan() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create an in-progress plan
        let mut plan = Plan::with_id("crashed-plan", "Crashed Plan", "/test.md");
        plan.set_status(PlanStatus::InProgress);
        manager.create_plan(plan).await.unwrap();

        let stats = scan_for_recovery(&manager).await.unwrap();

        assert_eq!(stats.plans_to_recover, 1);
        assert_eq!(stats.specs_to_recover, 0);
        assert_eq!(stats.executions_to_recover, 0);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_recovery_finds_running_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a running execution
        let mut exec = LoopExecution::with_id("crashed-exec", "ralph");
        exec.set_status(LoopExecutionStatus::Running);
        exec.increment_iteration();
        manager.create_execution(exec).await.unwrap();

        let stats = scan_for_recovery(&manager).await.unwrap();

        assert_eq!(stats.plans_to_recover, 0);
        assert_eq!(stats.executions_to_recover, 1);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_recovery_marks_as_paused() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create running execution
        let mut exec = LoopExecution::with_id("crashed-exec", "ralph");
        exec.set_status(LoopExecutionStatus::Running);
        manager.create_execution(exec).await.unwrap();

        // Mark as paused
        let count = mark_crashed_as_paused(&manager).await.unwrap();
        assert_eq!(count, 1);

        // Verify it's now paused
        let recovered = manager.get_execution("crashed-exec").await.unwrap().unwrap();
        assert_eq!(recovered.status, LoopExecutionStatus::Paused);
        assert!(recovered.last_error.is_some());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_scan_and_mark_recovery() {
        // Tests the recovery flow without sync (for in-memory test data)
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create crashed state
        let mut plan = Plan::with_id("plan-1", "Plan 1", "/p1.md");
        plan.set_status(PlanStatus::InProgress);
        manager.create_plan(plan).await.unwrap();

        let mut exec = LoopExecution::with_id("exec-1", "phase");
        exec.set_status(LoopExecutionStatus::Rebasing);
        manager.create_execution(exec).await.unwrap();

        // Scan for incomplete work
        let stats = scan_for_recovery(&manager).await.unwrap();
        assert_eq!(stats.plans_to_recover, 1);
        assert_eq!(stats.executions_to_recover, 1);

        // Mark crashed items as paused
        let count = mark_crashed_as_paused(&manager).await.unwrap();
        assert_eq!(count, 2);

        // Verify states were updated
        let plan = manager.get_plan("plan-1").await.unwrap().unwrap();
        assert_eq!(plan.status, PlanStatus::Ready);

        let exec = manager.get_execution("exec-1").await.unwrap().unwrap();
        assert_eq!(exec.status, LoopExecutionStatus::Paused);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_recover_with_sync() {
        // Tests the full recovery flow including sync
        // In this case, sync will clear the empty store (no JSONL files)
        // and recovery should report 0 items to recover
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Run recovery on empty store
        let stats = recover(&manager).await.unwrap();
        assert_eq!(stats.plans_to_recover, 0);
        assert_eq!(stats.specs_to_recover, 0);
        assert_eq!(stats.executions_to_recover, 0);

        manager.shutdown().await.unwrap();
    }
}
