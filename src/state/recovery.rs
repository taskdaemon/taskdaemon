//! Crash recovery
//!
//! Detects incomplete loops from TaskStore and prepares them for restart.

use tracing::{debug, info, warn};

use crate::domain::{Loop, LoopExecution, LoopExecutionStatus, LoopStatus};

use super::StateManager;

/// Recovery statistics
#[derive(Debug, Default)]
pub struct RecoveryStats {
    /// Number of loops (records) that need recovery
    pub loops_to_recover: usize,
    /// Number of executions that need recovery
    pub executions_to_recover: usize,
}

impl std::fmt::Display for RecoveryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "loops: {}, executions: {}",
            self.loops_to_recover, self.executions_to_recover
        )
    }
}

/// Check for incomplete work and gather recovery statistics
///
/// This scans the TaskStore for:
/// - Loops in InProgress state (crashed mid-execution)
/// - LoopExecutions in Running/Rebasing state (crashed mid-iteration)
pub async fn scan_for_recovery(state: &StateManager) -> eyre::Result<RecoveryStats> {
    debug!("scan_for_recovery: called");
    let mut stats = RecoveryStats::default();

    // Find loops that were in progress
    debug!("scan_for_recovery: listing in_progress loops");
    let in_progress_loops = state
        .list_loops(None, Some("in_progress".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list in-progress loops: {}", e))?;
    stats.loops_to_recover = in_progress_loops.len();

    for record in &in_progress_loops {
        debug!(loop_id = %record.id, loop_type = %record.r#type, "scan_for_recovery: found in-progress loop needing recovery");
    }

    // Find executions that were active (running or rebasing)
    debug!("scan_for_recovery: listing running executions");
    let running_execs = state
        .list_executions(Some("running".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list running executions: {}", e))?;

    debug!("scan_for_recovery: listing rebasing executions");
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
            "scan_for_recovery: found active execution needing recovery"
        );
    }

    if stats.loops_to_recover > 0 || stats.executions_to_recover > 0 {
        debug!("scan_for_recovery: incomplete work found");
        info!("Recovery scan found incomplete work: {}", stats);
    } else {
        debug!("scan_for_recovery: no incomplete work found");
    }

    Ok(stats)
}

/// Get all incomplete items that need recovery
pub async fn get_incomplete_items(state: &StateManager) -> eyre::Result<(Vec<Loop>, Vec<LoopExecution>)> {
    debug!("get_incomplete_items: called");
    // Get in-progress loops
    debug!("get_incomplete_items: listing in_progress loops");
    let loops = state
        .list_loops(None, Some("in_progress".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list in-progress loops: {}", e))?;
    debug!(count = loops.len(), "get_incomplete_items: found in_progress loops");

    // Get active executions
    debug!("get_incomplete_items: listing running executions");
    let mut executions = state
        .list_executions(Some("running".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list running executions: {}", e))?;
    debug!(
        count = executions.len(),
        "get_incomplete_items: found running executions"
    );

    debug!("get_incomplete_items: listing rebasing executions");
    let rebasing_execs = state
        .list_executions(Some("rebasing".to_string()), None)
        .await
        .map_err(|e| eyre::eyre!("Failed to list rebasing executions: {}", e))?;
    debug!(
        count = rebasing_execs.len(),
        "get_incomplete_items: found rebasing executions"
    );

    executions.extend(rebasing_execs);

    debug!(
        loops = loops.len(),
        executions = executions.len(),
        "get_incomplete_items: returning incomplete items"
    );
    Ok((loops, executions))
}

/// Mark crashed items as paused so they can be resumed
///
/// This transitions active items to appropriate states to indicate they
/// need to be resumed (rather than restarted from scratch).
pub async fn mark_crashed_as_paused(state: &StateManager) -> eyre::Result<usize> {
    debug!("mark_crashed_as_paused: called");
    let (loops, executions) = get_incomplete_items(state).await?;
    let mut count = 0;

    // Mark loops as ready (so they can be picked up again)
    debug!(
        loop_count = loops.len(),
        "mark_crashed_as_paused: processing crashed loops"
    );
    for mut record in loops {
        debug!(loop_id = %record.id, "mark_crashed_as_paused: marking loop as ready");
        warn!(loop_id = %record.id, loop_type = %record.r#type, "Marking crashed loop as ready for retry");
        record.set_status(LoopStatus::Ready);
        state
            .update_loop(record)
            .await
            .map_err(|e| eyre::eyre!("Failed to update loop: {}", e))?;
        count += 1;
    }

    // Mark executions as paused
    debug!(
        execution_count = executions.len(),
        "mark_crashed_as_paused: processing crashed executions"
    );
    for mut exec in executions {
        debug!(exec_id = %exec.id, "mark_crashed_as_paused: marking execution as paused");
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
        debug!(%count, "mark_crashed_as_paused: marked items as paused");
        info!("Marked {} crashed items as paused for recovery", count);
    } else {
        debug!("mark_crashed_as_paused: no items to mark");
    }

    Ok(count)
}

/// Full recovery process: scan, mark as paused, sync store
pub async fn recover(state: &StateManager) -> eyre::Result<RecoveryStats> {
    debug!("recover: called");
    info!("Starting crash recovery process");

    // First sync from JSONL files to ensure we have latest state
    debug!("recover: syncing store");
    state
        .sync()
        .await
        .map_err(|e| eyre::eyre!("Failed to sync store: {}", e))?;

    // Scan for incomplete work
    debug!("recover: scanning for recovery");
    let stats = scan_for_recovery(state).await?;

    // Mark crashed items as paused
    if stats.loops_to_recover > 0 || stats.executions_to_recover > 0 {
        debug!("recover: incomplete work found, marking as paused");
        mark_crashed_as_paused(state).await?;
    } else {
        debug!("recover: no incomplete work found");
    }

    debug!("recover: complete");
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

        assert_eq!(stats.loops_to_recover, 0);
        assert_eq!(stats.executions_to_recover, 0);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_recovery_finds_in_progress_loop() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create an in-progress loop
        let mut record = Loop::with_id("crashed-loop", "mytype", "Crashed Loop");
        record.set_status(LoopStatus::InProgress);
        manager.create_loop(record).await.unwrap();

        let stats = scan_for_recovery(&manager).await.unwrap();

        assert_eq!(stats.loops_to_recover, 1);
        assert_eq!(stats.executions_to_recover, 0);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_recovery_finds_running_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a running execution
        let mut exec = LoopExecution::with_id("crashed-exec", "mytype");
        exec.set_status(LoopExecutionStatus::Running);
        exec.increment_iteration();
        manager.create_execution(exec).await.unwrap();

        let stats = scan_for_recovery(&manager).await.unwrap();

        assert_eq!(stats.loops_to_recover, 0);
        assert_eq!(stats.executions_to_recover, 1);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_recovery_marks_as_paused() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create running execution
        let mut exec = LoopExecution::with_id("crashed-exec", "mytype");
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
        let mut record = Loop::with_id("loop-1", "mytype", "Loop 1");
        record.set_status(LoopStatus::InProgress);
        manager.create_loop(record).await.unwrap();

        let mut exec = LoopExecution::with_id("exec-1", "mytype");
        exec.set_status(LoopExecutionStatus::Rebasing);
        manager.create_execution(exec).await.unwrap();

        // Scan for incomplete work
        let stats = scan_for_recovery(&manager).await.unwrap();
        assert_eq!(stats.loops_to_recover, 1);
        assert_eq!(stats.executions_to_recover, 1);

        // Mark crashed items as paused
        let count = mark_crashed_as_paused(&manager).await.unwrap();
        assert_eq!(count, 2);

        // Verify states were updated
        let record = manager.get_loop("loop-1").await.unwrap().unwrap();
        assert_eq!(record.status, LoopStatus::Ready);

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
        assert_eq!(stats.loops_to_recover, 0);
        assert_eq!(stats.executions_to_recover, 0);

        manager.shutdown().await.unwrap();
    }
}
