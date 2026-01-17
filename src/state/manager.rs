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

/// Handle to send commands to the StateManager
#[derive(Clone)]
pub struct StateManager {
    tx: mpsc::Sender<StateCommand>,
}

impl StateManager {
    /// Spawn a new StateManager actor
    pub fn spawn(store_path: impl AsRef<Path>) -> eyre::Result<Self> {
        let store = Store::open(store_path.as_ref())?;

        let (tx, rx) = mpsc::channel(256);

        // Spawn the actor task
        tokio::spawn(actor_loop(store, rx));

        info!("StateManager spawned");

        Ok(Self { tx })
    }

    // === Loop operations (generic work units) ===

    /// Create a new Loop record
    pub async fn create_loop(&self, record: Loop) -> StateResponse<String> {
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
        self.get_loop(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Loop {}", id)))
    }

    /// List all Loop records for a given parent ID
    pub async fn list_loops_for_parent(&self, parent_id: &str) -> StateResponse<Vec<Loop>> {
        self.list_loops(None, None, Some(parent_id.to_string())).await
    }

    /// List all Loop records of a given type
    pub async fn list_loops_by_type(&self, loop_type: &str) -> StateResponse<Vec<Loop>> {
        self.list_loops(Some(loop_type.to_string()), None, None).await
    }

    // === LoopExecution operations ===

    /// Create a new LoopExecution
    pub async fn create_execution(&self, execution: LoopExecution) -> StateResponse<String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::CreateExecution {
                execution,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Get a LoopExecution by ID
    pub async fn get_execution(&self, id: &str) -> StateResponse<Option<LoopExecution>> {
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
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::UpdateExecution {
                execution,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// List LoopExecutions with optional filters
    pub async fn list_executions(
        &self,
        status_filter: Option<String>,
        loop_type_filter: Option<String>,
    ) -> StateResponse<Vec<LoopExecution>> {
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
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::Sync { reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Rebuild indexes for all record types
    pub async fn rebuild_indexes(&self) -> StateResponse<usize> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::RebuildIndexes { reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Shutdown the StateManager
    pub async fn shutdown(&self) -> Result<(), StateError> {
        self.tx
            .send(StateCommand::Shutdown)
            .await
            .map_err(|_| StateError::ChannelError)
    }

    // === Convenience methods ===

    /// Get aggregated metrics from all loop executions
    pub async fn get_metrics(&self) -> eyre::Result<DaemonMetrics> {
        let executions = self.list_executions(None, None).await?;

        let mut metrics = DaemonMetrics::default();

        for exec in executions {
            metrics.total_executions += 1;
            match exec.status {
                LoopExecutionStatus::Draft => metrics.drafts += 1,
                LoopExecutionStatus::Running => metrics.running += 1,
                LoopExecutionStatus::Pending => metrics.pending += 1,
                LoopExecutionStatus::Complete => metrics.completed += 1,
                LoopExecutionStatus::Failed => metrics.failed += 1,
                LoopExecutionStatus::Paused => metrics.paused += 1,
                LoopExecutionStatus::Stopped => metrics.stopped += 1,
                LoopExecutionStatus::Rebasing | LoopExecutionStatus::Blocked => {}
            }
            metrics.total_iterations += exec.iteration as u64;
        }

        Ok(metrics)
    }

    /// Create a new LoopExecution (alias for create_execution)
    pub async fn create_loop_execution(&self, execution: LoopExecution) -> StateResponse<String> {
        self.create_execution(execution).await
    }

    /// Get a LoopExecution for a specific record (by parent field)
    pub async fn get_loop_execution_for_spec(&self, record_id: &str) -> StateResponse<Option<LoopExecution>> {
        // List all executions and find one with matching parent
        let executions = self.list_executions(None, None).await?;
        Ok(executions.into_iter().find(|e| e.parent.as_deref() == Some(record_id)))
    }

    // === Execution control methods ===

    /// Cancel a running execution (sets status to Stopped)
    pub async fn cancel_execution(&self, id: &str) -> StateResponse<()> {
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if execution.is_terminal() {
            return Err(StateError::StoreError("Cannot cancel a terminal execution".to_string()));
        }

        execution.set_status(LoopExecutionStatus::Stopped);
        self.update_execution(execution).await
    }

    /// Pause a running execution
    pub async fn pause_execution(&self, id: &str) -> StateResponse<()> {
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if execution.status != LoopExecutionStatus::Running {
            return Err(StateError::StoreError("Can only pause running executions".to_string()));
        }

        execution.set_status(LoopExecutionStatus::Paused);
        self.update_execution(execution).await
    }

    /// Resume a paused execution
    pub async fn resume_execution(&self, id: &str) -> StateResponse<()> {
        let mut execution = self
            .get_execution(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Execution {}", id)))?;

        if !execution.is_resumable() {
            return Err(StateError::StoreError(
                "Can only resume paused or blocked executions".to_string(),
            ));
        }

        execution.set_status(LoopExecutionStatus::Running);
        self.update_execution(execution).await
    }
}

/// The actor loop that owns the Store and processes commands
async fn actor_loop(mut store: Store, mut rx: mpsc::Receiver<StateCommand>) {
    debug!("StateManager actor started");

    while let Some(cmd) = rx.recv().await {
        match cmd {
            // Loop operations (generic work units)
            StateCommand::CreateLoop { record, reply } => {
                let result = store.create(record).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::GetLoop { id, reply } => {
                let result: StateResponse<Option<Loop>> =
                    store.get(&id).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::UpdateLoop { record, reply } => {
                let result = store.update(record).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::ListLoops {
                type_filter,
                status_filter,
                parent_filter,
                reply,
            } => {
                let mut filters = Vec::new();
                if let Some(loop_type) = type_filter {
                    filters.push(Filter {
                        field: "type".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(loop_type),
                    });
                }
                if let Some(status) = status_filter {
                    filters.push(Filter {
                        field: "status".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(status),
                    });
                }
                if let Some(parent) = parent_filter {
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
                let result = store
                    .create(execution)
                    .map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::GetExecution { id, reply } => {
                let result: StateResponse<Option<LoopExecution>> =
                    store.get(&id).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::UpdateExecution { execution, reply } => {
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
                let mut filters = Vec::new();
                if let Some(status) = status_filter {
                    filters.push(Filter {
                        field: "status".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(status),
                    });
                }
                if let Some(loop_type) = loop_type_filter {
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
                let result = store
                    .delete::<Loop>(&id)
                    .map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::DeleteExecution { id, reply } => {
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
                // Generic get is not implemented for now
                let _ = reply.send(Err(StateError::StoreError("Generic get not implemented".to_string())));
            }

            StateCommand::Sync { reply } => {
                let result = store.sync().map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::RebuildIndexes { reply } => {
                let mut count = 0;
                if let Ok(c) = store.rebuild_indexes::<Loop>() {
                    count += c;
                }
                if let Ok(c) = store.rebuild_indexes::<LoopExecution>() {
                    count += c;
                }
                let _ = reply.send(Ok(count));
            }

            StateCommand::Shutdown => {
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
}
