//! StateManager - actor that owns TaskStore
//!
//! Processes commands via channels for thread-safe access to persistent state.

use std::path::Path;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::domain::{Filter, FilterOp, IndexValue, Loop, LoopExecution, LoopExecutionStatus, Store};

use super::messages::{StateCommand, StateError, StateResponse};

/// Aggregated metrics from the daemon's state
#[derive(Debug, Default, serde::Serialize)]
pub struct DaemonMetrics {
    /// Total number of loop executions
    pub total_executions: u64,
    /// Draft plans awaiting approval
    pub drafts: u64,
    /// Currently running loops
    pub running: u64,
    /// Loops waiting to start
    pub pending: u64,
    /// Successfully completed loops
    pub completed: u64,
    /// Failed loops
    pub failed: u64,
    /// Paused loops
    pub paused: u64,
    /// Stopped loops
    pub stopped: u64,
    /// Total iterations across all loops
    pub total_iterations: u64,
}

/// Event broadcast when state changes that TUI should react to
#[derive(Debug, Clone)]
pub enum StateEvent {
    /// A new execution was created (e.g., cascade spawned a child)
    ExecutionCreated { id: String, loop_type: String },
    /// An execution status changed
    ExecutionUpdated { id: String },
}

/// Path to the state change notification file
/// This file contains a monotonically increasing counter that's bumped on every state change.
/// External processes can poll this file to detect when they should refresh.
fn state_notify_path() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("taskdaemon")
        .join(".state_version")
}

/// Bump the state version to notify other processes of changes
fn notify_state_change() {
    let path = state_notify_path();

    // Read current version, increment, write back
    let version: u64 = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    if let Err(e) = std::fs::write(&path, format!("{}", version + 1)) {
        tracing::debug!(error = %e, "Failed to write state notification file");
    }
}

/// Read the current state version (for external processes to poll)
pub fn read_state_version() -> u64 {
    std::fs::read_to_string(state_notify_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Handle to send commands to the StateManager
#[derive(Clone)]
pub struct StateManager {
    tx: mpsc::Sender<StateCommand>,
    /// Broadcast sender for state change notifications
    event_tx: tokio::sync::broadcast::Sender<StateEvent>,
}

impl StateManager {
    /// Spawn a new StateManager actor
    pub fn spawn(store_path: impl AsRef<Path>) -> eyre::Result<Self> {
        debug!(store_path = %store_path.as_ref().display(), "spawn: called");
        let mut store = Store::open(store_path.as_ref())?;

        // Rebuild indexes for all record types after sync
        // This ensures status-based queries work correctly
        let loop_count = store.rebuild_indexes::<Loop>()?;
        let exec_count = store.rebuild_indexes::<LoopExecution>()?;
        info!(
            loop_count,
            exec_count, "Rebuilt indexes for Loop and LoopExecution records"
        );

        let (tx, rx) = mpsc::channel(256);

        // Broadcast channel for state change notifications (TUI subscribes)
        let (event_tx, _) = tokio::sync::broadcast::channel(64);

        // Spawn the actor task
        tokio::spawn(actor_loop(store, rx));

        info!("StateManager spawned");

        Ok(Self { tx, event_tx })
    }

    /// Subscribe to state change events (for instant TUI updates)
    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<StateEvent> {
        self.event_tx.subscribe()
    }

    // === Loop operations (generic work units) ===

    /// Create a new Loop record
    pub async fn create_loop(&self, record: Loop) -> StateResponse<String> {
        debug!(record_id = %record.id, record_type = %record.r#type, "create_loop: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::CreateLoop {
                record,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Get a Loop record by ID
    pub async fn get_loop(&self, id: &str) -> StateResponse<Option<Loop>> {
        debug!(%id, "get_loop: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::GetLoop {
                id: id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Update a Loop record
    pub async fn update_loop(&self, record: Loop) -> StateResponse<()> {
        debug!(record_id = %record.id, record_status = ?record.status, "update_loop: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::UpdateLoop {
                record,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// List Loop records with optional filters
    pub async fn list_loops(
        &self,
        type_filter: Option<String>,
        status_filter: Option<String>,
        parent_filter: Option<String>,
    ) -> StateResponse<Vec<Loop>> {
        debug!(?type_filter, ?status_filter, ?parent_filter, "list_loops: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::ListLoops {
                type_filter,
                status_filter,
                parent_filter,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Get a Loop record by ID, returning error if not found
    pub async fn get_loop_required(&self, id: &str) -> Result<Loop, StateError> {
        debug!(%id, "get_loop_required: called");
        self.get_loop(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Loop {}", id)))
    }

    /// List all Loop records for a given parent ID
    pub async fn list_loops_for_parent(&self, parent_id: &str) -> StateResponse<Vec<Loop>> {
        debug!(%parent_id, "list_loops_for_parent: called");
        self.list_loops(None, None, Some(parent_id.to_string())).await
    }

    /// List all Loop records of a given type
    pub async fn list_loops_by_type(&self, loop_type: &str) -> StateResponse<Vec<Loop>> {
        debug!(%loop_type, "list_loops_by_type: called");
        self.list_loops(Some(loop_type.to_string()), None, None).await
    }

    // === LoopExecution operations ===

    /// Create a new LoopExecution
    pub async fn create_execution(&self, execution: LoopExecution) -> StateResponse<String> {
        debug!(execution_id = %execution.id, loop_type = %execution.loop_type, "create_execution: called");
        let exec_id = execution.id.clone();
        let loop_type = execution.loop_type.clone();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::CreateExecution {
                execution,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        let result = reply_rx.await.map_err(|_| StateError::ChannelError)?;

        // Broadcast event so TUI can update immediately (same-process)
        // Also notify via file for cross-process updates
        if result.is_ok() {
            let _ = self
                .event_tx
                .send(StateEvent::ExecutionCreated { id: exec_id, loop_type });
            notify_state_change();
        }

        result
    }

    /// Get a LoopExecution by ID
    pub async fn get_execution(&self, id: &str) -> StateResponse<Option<LoopExecution>> {
        debug!(%id, "get_execution: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::GetExecution {
                id: id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Update a LoopExecution
    pub async fn update_execution(&self, execution: LoopExecution) -> StateResponse<()> {
        debug!(execution_id = %execution.id, status = ?execution.status, "update_execution: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::UpdateExecution {
                execution,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        let result = reply_rx.await.map_err(|_| StateError::ChannelError)?;

        // Notify other processes of state change
        if result.is_ok() {
            notify_state_change();
        }

        result
    }

    /// List LoopExecutions with optional filters
    pub async fn list_executions(
        &self,
        status_filter: Option<String>,
        loop_type_filter: Option<String>,
    ) -> StateResponse<Vec<LoopExecution>> {
        debug!(?status_filter, ?loop_type_filter, "list_executions: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::ListExecutions {
                status_filter,
                loop_type_filter,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    // === Delete operations ===

    /// Delete a Loop record by ID
    pub async fn delete_loop(&self, id: &str) -> StateResponse<()> {
        debug!(%id, "delete_loop: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::DeleteLoop {
                id: id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Delete a LoopExecution by ID
    pub async fn delete_execution(&self, id: &str) -> StateResponse<()> {
        debug!(%id, "delete_execution: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::DeleteExecution {
                id: id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Sync the store from JSONL files
    pub async fn sync(&self) -> StateResponse<()> {
        debug!("sync: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::Sync { reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Rebuild indexes for all record types
    pub async fn rebuild_indexes(&self) -> StateResponse<usize> {
        debug!("rebuild_indexes: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::RebuildIndexes { reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Shutdown the StateManager
    pub async fn shutdown(&self) -> Result<(), StateError> {
        debug!("shutdown: called");
        self.tx
            .send(StateCommand::Shutdown)
            .await
            .map_err(|_| StateError::ChannelError)
    }

    // === Convenience methods ===

    /// Get aggregated metrics from all loop executions
    pub async fn get_metrics(&self) -> eyre::Result<DaemonMetrics> {
        debug!("get_metrics: called");
        let executions = self.list_executions(None, None).await?;

        let mut metrics = DaemonMetrics::default();

        for exec in executions {
            metrics.total_executions += 1;
            match exec.status {
                LoopExecutionStatus::Draft => {
                    debug!("get_metrics: status is Draft");
                    metrics.drafts += 1;
                }
                LoopExecutionStatus::Running => {
                    debug!("get_metrics: status is Running");
                    metrics.running += 1;
                }
                LoopExecutionStatus::Pending => {
                    debug!("get_metrics: status is Pending");
                    metrics.pending += 1;
                }
                LoopExecutionStatus::Complete => {
                    debug!("get_metrics: status is Complete");
                    metrics.completed += 1;
                }
                LoopExecutionStatus::Failed => {
                    debug!("get_metrics: status is Failed");
                    metrics.failed += 1;
                }
                LoopExecutionStatus::Paused => {
                    debug!("get_metrics: status is Paused");
                    metrics.paused += 1;
                }
                LoopExecutionStatus::Stopped => {
                    debug!("get_metrics: status is Stopped");
                    metrics.stopped += 1;
                }
                LoopExecutionStatus::Rebasing | LoopExecutionStatus::Blocked => {
                    debug!("get_metrics: status is Rebasing or Blocked");
                }
            }
            metrics.total_iterations += exec.iteration as u64;
        }

        Ok(metrics)
    }

    /// Create a new LoopExecution (alias for create_execution)
    pub async fn create_loop_execution(&self, execution: LoopExecution) -> StateResponse<String> {
        debug!(execution_id = %execution.id, "create_loop_execution: called");
        self.create_execution(execution).await
    }

    /// Get a LoopExecution for a specific record (by parent field)
    pub async fn get_loop_execution_for_spec(&self, record_id: &str) -> StateResponse<Option<LoopExecution>> {
        debug!(%record_id, "get_loop_execution_for_spec: called");
        // List all executions and find one with matching parent
        let executions = self.list_executions(None, None).await?;
        Ok(executions.into_iter().find(|e| e.parent.as_deref() == Some(record_id)))
    }

    // === Execution control methods ===

    /// Cancel a running execution (sets status to Stopped)
    pub async fn cancel_execution(&self, id: &str) -> StateResponse<()> {
        debug!(%id, "cancel_execution: called");
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if execution.is_terminal() {
            debug!("cancel_execution: execution is terminal, cannot cancel");
            return Err(StateError::StoreError("Cannot cancel a terminal execution".to_string()));
        }

        debug!("cancel_execution: setting status to Stopped");
        execution.set_status(LoopExecutionStatus::Stopped);
        self.update_execution(execution).await
    }

    /// Pause a running execution
    pub async fn pause_execution(&self, id: &str) -> StateResponse<()> {
        debug!(%id, "pause_execution: called");
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if execution.status != LoopExecutionStatus::Running {
            debug!("pause_execution: execution not running, cannot pause");
            return Err(StateError::StoreError("Can only pause running executions".to_string()));
        }

        debug!("pause_execution: setting status to Paused");
        execution.set_status(LoopExecutionStatus::Paused);
        self.update_execution(execution).await
    }

    /// Resume a paused execution
    pub async fn resume_execution(&self, id: &str) -> StateResponse<()> {
        debug!(%id, "resume_execution: called");
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if !execution.is_resumable() {
            debug!("resume_execution: execution not resumable");
            return Err(StateError::StoreError(
                "Can only resume paused or blocked executions".to_string(),
            ));
        }

        debug!("resume_execution: setting status to Running");
        execution.set_status(LoopExecutionStatus::Running);
        self.update_execution(execution).await
    }

    /// Start a draft execution (transitions Draft -> Pending, daemon picks it up)
    pub async fn start_draft(&self, id: &str) -> StateResponse<()> {
        debug!(%id, "start_draft: called");
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if !execution.is_draft() {
            debug!("start_draft: execution is not draft, cannot start");
            return Err(StateError::StoreError("Can only start draft executions".to_string()));
        }

        debug!("start_draft: marking execution as ready");
        execution.mark_ready();
        self.update_execution(execution).await
    }

    /// Activate a draft execution (transitions Draft -> Running directly, no pending state)
    pub async fn activate_draft(&self, id: &str) -> StateResponse<()> {
        debug!(%id, "activate_draft: called");
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if !execution.is_draft() {
            debug!("activate_draft: execution is not draft, cannot activate");
            return Err(StateError::StoreError("Can only activate draft executions".to_string()));
        }

        debug!("activate_draft: setting status to Pending for LoopManager pickup");
        execution.set_status(LoopExecutionStatus::Pending);
        self.update_execution(execution).await
    }
}

/// The actor loop that owns the Store and processes commands
async fn actor_loop(mut store: Store, mut rx: mpsc::Receiver<StateCommand>) {
    debug!("actor_loop: called");
    debug!("StateManager actor started");

    while let Some(cmd) = rx.recv().await {
        match cmd {
            // Loop operations (generic work units)
            StateCommand::CreateLoop { record, reply } => {
                debug!(record_id = %record.id, "actor_loop: CreateLoop command");
                let result = store.create(record).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::GetLoop { id, reply } => {
                debug!(%id, "actor_loop: GetLoop command");
                let result: StateResponse<Option<Loop>> =
                    store.get(&id).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::UpdateLoop { record, reply } => {
                debug!(record_id = %record.id, "actor_loop: UpdateLoop command");
                let result = store.update(record).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::ListLoops {
                type_filter,
                status_filter,
                parent_filter,
                reply,
            } => {
                debug!(
                    ?type_filter,
                    ?status_filter,
                    ?parent_filter,
                    "actor_loop: ListLoops command"
                );
                let mut filters = Vec::new();
                if let Some(loop_type) = type_filter {
                    debug!(%loop_type, "actor_loop: ListLoops adding type filter");
                    filters.push(Filter {
                        field: "type".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(loop_type),
                    });
                }
                if let Some(status) = status_filter {
                    debug!(%status, "actor_loop: ListLoops adding status filter");
                    filters.push(Filter {
                        field: "status".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(status),
                    });
                }
                if let Some(parent) = parent_filter {
                    debug!(%parent, "actor_loop: ListLoops adding parent filter");
                    filters.push(Filter {
                        field: "parent".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(parent),
                    });
                }

                let result: StateResponse<Vec<Loop>> =
                    store.list(&filters).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::CreateExecution { execution, reply } => {
                debug!(execution_id = %execution.id, "actor_loop: CreateExecution command");
                let result = store
                    .create(execution)
                    .map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::GetExecution { id, reply } => {
                debug!(%id, "actor_loop: GetExecution command");
                let result: StateResponse<Option<LoopExecution>> =
                    store.get(&id).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::UpdateExecution { execution, reply } => {
                debug!(execution_id = %execution.id, "actor_loop: UpdateExecution command");
                let result = store
                    .update(execution)
                    .map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::ListExecutions {
                status_filter,
                loop_type_filter,
                reply,
            } => {
                debug!(?status_filter, ?loop_type_filter, "actor_loop: ListExecutions command");
                let mut filters = Vec::new();
                if let Some(status) = status_filter {
                    debug!(%status, "actor_loop: ListExecutions adding status filter");
                    filters.push(Filter {
                        field: "status".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(status),
                    });
                }
                if let Some(loop_type) = loop_type_filter {
                    debug!(%loop_type, "actor_loop: ListExecutions adding loop_type filter");
                    filters.push(Filter {
                        field: "loop_type".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(loop_type),
                    });
                }

                let result: StateResponse<Vec<LoopExecution>> =
                    store.list(&filters).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::DeleteLoop { id, reply } => {
                debug!(%id, "actor_loop: DeleteLoop command");
                let result = store
                    .delete::<Loop>(&id)
                    .map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::DeleteExecution { id, reply } => {
                debug!(%id, "actor_loop: DeleteExecution command");
                let result = store
                    .delete::<LoopExecution>(&id)
                    .map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::GetGeneric {
                collection: _,
                id: _,
                reply,
            } => {
                debug!("actor_loop: GetGeneric command (not implemented)");
                // Generic get is not implemented for now
                let _ = reply.send(Err(StateError::StoreError("Generic get not implemented".to_string())));
            }

            StateCommand::Sync { reply } => {
                debug!("actor_loop: Sync command");
                let result = store.sync().map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::RebuildIndexes { reply } => {
                debug!("actor_loop: RebuildIndexes command");
                let mut count = 0;
                if let Ok(c) = store.rebuild_indexes::<Loop>() {
                    debug!(count = c, "actor_loop: RebuildIndexes Loop indexes rebuilt");
                    count += c;
                }
                if let Ok(c) = store.rebuild_indexes::<LoopExecution>() {
                    debug!(count = c, "actor_loop: RebuildIndexes LoopExecution indexes rebuilt");
                    count += c;
                }
                let _ = reply.send(Ok(count));
            }

            StateCommand::Shutdown => {
                debug!("actor_loop: Shutdown command");
                info!("StateManager shutting down");
                break;
            }
        }
    }

    debug!("StateManager actor stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_state_manager_loop_crud() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create
        let record = Loop::with_id("test-loop", "mytype", "Test Loop");
        let id = manager.create_loop(record.clone()).await.unwrap();
        assert_eq!(id, "test-loop");

        // Get
        let retrieved = manager.get_loop("test-loop").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().title, "Test Loop");

        // Update
        let mut updated = record.clone();
        updated.set_status(crate::domain::LoopStatus::Ready);
        manager.update_loop(updated).await.unwrap();

        let retrieved = manager.get_loop("test-loop").await.unwrap().unwrap();
        assert_eq!(retrieved.status, crate::domain::LoopStatus::Ready);

        // List
        let loops = manager.list_loops(None, None, None).await.unwrap();
        assert_eq!(loops.len(), 1);

        // Shutdown
        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_state_manager_execution_crud() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create execution
        let exec = LoopExecution::with_id("test-exec", "mytype");
        let id = manager.create_execution(exec).await.unwrap();
        assert_eq!(id, "test-exec");

        // Get
        let retrieved = manager.get_execution("test-exec").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().loop_type, "mytype");

        // List by type
        let execs = manager.list_executions(None, Some("mytype".to_string())).await.unwrap();
        assert_eq!(execs.len(), 1);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_state_manager_get_nonexistent() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        let result = manager.get_loop("nonexistent").await.unwrap();
        assert!(result.is_none());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_state_manager_list_with_filter() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create loops with different statuses
        let mut loop1 = Loop::with_id("loop-1", "mytype", "Loop 1");
        loop1.set_status(crate::domain::LoopStatus::Pending);
        manager.create_loop(loop1).await.unwrap();

        let mut loop2 = Loop::with_id("loop-2", "mytype", "Loop 2");
        loop2.set_status(crate::domain::LoopStatus::Ready);
        manager.create_loop(loop2).await.unwrap();

        // List with filter
        let pending_loops = manager
            .list_loops(None, Some("pending".to_string()), None)
            .await
            .unwrap();
        assert_eq!(pending_loops.len(), 1);
        assert_eq!(pending_loops[0].id, "loop-1");

        manager.shutdown().await.unwrap();
    }

    // === POSITIVE TESTS: start_draft ===

    #[tokio::test]
    async fn test_start_draft_transitions_draft_to_pending() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a draft execution
        let mut exec = LoopExecution::with_id("draft-exec", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Draft);
        manager.create_execution(exec).await.unwrap();

        // Verify it's in draft status
        let retrieved = manager.get_execution("draft-exec").await.unwrap().unwrap();
        assert_eq!(retrieved.status, crate::domain::LoopExecutionStatus::Draft);

        // Start the draft
        manager.start_draft("draft-exec").await.unwrap();

        // Verify it's now pending
        let updated = manager.get_execution("draft-exec").await.unwrap().unwrap();
        assert_eq!(updated.status, crate::domain::LoopExecutionStatus::Pending);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_draft_updates_timestamp() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a draft execution
        let mut exec = LoopExecution::with_id("draft-exec-2", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Draft);
        let original_updated = exec.updated_at;
        manager.create_execution(exec).await.unwrap();

        // Small delay to ensure timestamp changes
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Start the draft
        manager.start_draft("draft-exec-2").await.unwrap();

        // Verify updated_at changed
        let updated = manager.get_execution("draft-exec-2").await.unwrap().unwrap();
        assert!(updated.updated_at > original_updated);

        manager.shutdown().await.unwrap();
    }

    // === NEGATIVE TESTS: start_draft ===

    #[tokio::test]
    async fn test_start_draft_fails_for_nonexistent_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        let result = manager.start_draft("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StateError::NotFound(_)));

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_draft_fails_for_running_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a running execution
        let mut exec = LoopExecution::with_id("running-exec", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Running);
        manager.create_execution(exec).await.unwrap();

        // Try to start it (should fail)
        let result = manager.start_draft("running-exec").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StateError::StoreError(_)));

        // Verify status unchanged
        let retrieved = manager.get_execution("running-exec").await.unwrap().unwrap();
        assert_eq!(retrieved.status, crate::domain::LoopExecutionStatus::Running);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_draft_fails_for_pending_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a pending execution
        let mut exec = LoopExecution::with_id("pending-exec", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Pending);
        manager.create_execution(exec).await.unwrap();

        // Try to start it (should fail - already pending)
        let result = manager.start_draft("pending-exec").await;
        assert!(result.is_err());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_draft_fails_for_complete_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a complete execution
        let mut exec = LoopExecution::with_id("complete-exec", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Complete);
        manager.create_execution(exec).await.unwrap();

        let result = manager.start_draft("complete-exec").await;
        assert!(result.is_err());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_draft_fails_for_failed_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a failed execution
        let mut exec = LoopExecution::with_id("failed-exec", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Failed);
        manager.create_execution(exec).await.unwrap();

        let result = manager.start_draft("failed-exec").await;
        assert!(result.is_err());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_draft_fails_for_paused_execution() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a paused execution
        let mut exec = LoopExecution::with_id("paused-exec", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Paused);
        manager.create_execution(exec).await.unwrap();

        let result = manager.start_draft("paused-exec").await;
        assert!(result.is_err());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_draft_idempotent_check() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create a draft execution
        let mut exec = LoopExecution::with_id("draft-idem", "plan");
        exec.set_status(crate::domain::LoopExecutionStatus::Draft);
        manager.create_execution(exec).await.unwrap();

        // Start once
        manager.start_draft("draft-idem").await.unwrap();

        // Try to start again (should fail - no longer draft)
        let result = manager.start_draft("draft-idem").await;
        assert!(result.is_err());

        manager.shutdown().await.unwrap();
    }
}
