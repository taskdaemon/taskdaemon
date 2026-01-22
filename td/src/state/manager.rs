//! StateManager - actor that owns TaskStore
//!
//! Processes commands via channels for thread-safe access to persistent state.

use std::path::Path;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::domain::{Filter, FilterOp, IndexValue, IterationLog, Loop, LoopExecution, LoopExecutionStatus, Store};
use crate::ipc::DaemonClient;

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
    /// An execution is now pending and ready for pickup by LoopManager
    ExecutionPending { id: String },
    /// A new iteration log was created
    IterationLogCreated {
        execution_id: String,
        iteration: u32,
        exit_code: i32,
    },
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
        let iter_log_count = store.rebuild_indexes::<IterationLog>()?;
        info!(
            loop_count,
            exec_count, iter_log_count, "Rebuilt indexes for Loop, LoopExecution, and IterationLog records"
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

    /// Notify daemon via IPC that an execution is pending
    ///
    /// This is fire-and-forget: errors are logged but not propagated to avoid
    /// blocking the TUI. The daemon's 60-second poll serves as a fallback.
    async fn notify_daemon_pending(&self, id: &str) {
        debug!(%id, "notify_daemon_pending: sending IPC notification");
        let client = DaemonClient::new();
        if let Err(e) = client.notify_pending(id).await {
            debug!(error = %e, %id, "notify_daemon_pending: could not notify daemon (may be running in same process)");
        }
    }

    /// Notify daemon via IPC that an execution was resumed
    async fn notify_daemon_resumed(&self, id: &str) {
        debug!(%id, "notify_daemon_resumed: sending IPC notification");
        let client = DaemonClient::new();
        if let Err(e) = client.notify_resumed(id).await {
            debug!(error = %e, %id, "notify_daemon_resumed: could not notify daemon (may be running in same process)");
        }
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
        let is_pending = execution.status == LoopExecutionStatus::Pending;

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
            let _ = self.event_tx.send(StateEvent::ExecutionCreated {
                id: exec_id.clone(),
                loop_type,
            });
            notify_state_change();

            // If created with Pending status, also notify LoopManager for immediate pickup
            // This happens when cascade creates child executions
            if is_pending {
                let _ = self.event_tx.send(StateEvent::ExecutionPending { id: exec_id });
            }
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

    /// Delete a LoopExecution by ID (also deletes associated IterationLogs)
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

    // === IterationLog operations ===

    /// Create a new IterationLog
    pub async fn create_iteration_log(&self, log: IterationLog) -> StateResponse<String> {
        debug!(log_id = %log.id, execution_id = %log.execution_id, iteration = log.iteration, "create_iteration_log: called");
        let execution_id = log.execution_id.clone();
        let iteration = log.iteration;
        let exit_code = log.exit_code;

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::CreateIterationLog { log, reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        let result = reply_rx.await.map_err(|_| StateError::ChannelError)?;

        // Broadcast event so TUI can update immediately
        if result.is_ok() {
            let _ = self.event_tx.send(StateEvent::IterationLogCreated {
                execution_id,
                iteration,
                exit_code,
            });
            notify_state_change();
        }

        result
    }

    /// List IterationLogs for a given execution (ordered by iteration number)
    pub async fn list_iteration_logs(&self, execution_id: &str) -> StateResponse<Vec<IterationLog>> {
        debug!(%execution_id, "list_iteration_logs: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::ListIterationLogs {
                execution_id: execution_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Get a specific IterationLog by ID
    pub async fn get_iteration_log(&self, id: &str) -> StateResponse<Option<IterationLog>> {
        debug!(%id, "get_iteration_log: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::GetIterationLog {
                id: id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Delete all IterationLogs for a given execution
    pub async fn delete_iteration_logs(&self, execution_id: &str) -> StateResponse<usize> {
        debug!(%execution_id, "delete_iteration_logs: called");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::DeleteIterationLogs {
                execution_id: execution_id.to_string(),
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
        let exec_id = execution.id.clone();
        let result = self.update_execution(execution).await;

        // Notify daemon via IPC for immediate pickup (fire-and-forget)
        if result.is_ok() {
            self.notify_daemon_resumed(&exec_id).await;
        }

        result
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
        let exec_id = execution.id.clone();
        let result = self.update_execution(execution).await;

        // Notify daemon via IPC for immediate pickup (fire-and-forget)
        // Also send in-process event for same-process daemon
        if result.is_ok() {
            let _ = self.event_tx.send(StateEvent::ExecutionPending { id: exec_id.clone() });
            self.notify_daemon_pending(&exec_id).await;
        }

        result
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
        let exec_id = execution.id.clone();
        let result = self.update_execution(execution).await;

        // Notify LoopManager that work is ready for immediate pickup
        // Both in-process event (for same-process daemon) and IPC (for separate daemon)
        if result.is_ok() {
            let _ = self.event_tx.send(StateEvent::ExecutionPending { id: exec_id.clone() });
            self.notify_daemon_pending(&exec_id).await;
        }

        result
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
                // Cascade delete: first delete all associated IterationLogs
                if let Ok(count) = store.delete_by_index::<IterationLog>("execution_id", IndexValue::String(id.clone()))
                {
                    debug!(count, %id, "actor_loop: DeleteExecution cascade deleted IterationLogs");
                }
                // Then delete the execution itself
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

            // IterationLog operations
            StateCommand::CreateIterationLog { log, reply } => {
                debug!(log_id = %log.id, "actor_loop: CreateIterationLog command");
                let result = store.create(log).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::ListIterationLogs { execution_id, reply } => {
                debug!(%execution_id, "actor_loop: ListIterationLogs command");
                let filters = vec![Filter {
                    field: "execution_id".to_string(),
                    op: FilterOp::Eq,
                    value: IndexValue::String(execution_id),
                }];
                let result: StateResponse<Vec<IterationLog>> =
                    store.list(&filters).map_err(|e| StateError::StoreError(e.to_string()));
                // Sort by iteration number (ascending)
                let result = result.map(|mut logs| {
                    logs.sort_by_key(|l| l.iteration);
                    logs
                });
                let _ = reply.send(result);
            }

            StateCommand::GetIterationLog { id, reply } => {
                debug!(%id, "actor_loop: GetIterationLog command");
                let result: StateResponse<Option<IterationLog>> =
                    store.get(&id).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::DeleteIterationLogs { execution_id, reply } => {
                debug!(%execution_id, "actor_loop: DeleteIterationLogs command");
                let result = store
                    .delete_by_index::<IterationLog>("execution_id", IndexValue::String(execution_id))
                    .map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
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
                if let Ok(c) = store.rebuild_indexes::<IterationLog>() {
                    debug!(count = c, "actor_loop: RebuildIndexes IterationLog indexes rebuilt");
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

    // === IterationLog tests ===

    #[tokio::test]
    async fn test_iteration_log_crud() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create an execution first
        let exec = LoopExecution::with_id("test-exec", "ralph");
        manager.create_execution(exec).await.unwrap();

        // Create an iteration log
        let log = IterationLog::new("test-exec", 1)
            .with_validation_command("otto ci")
            .with_exit_code(0)
            .with_stdout("All tests passed")
            .with_duration_ms(5000);

        let id = manager.create_iteration_log(log).await.unwrap();
        assert_eq!(id, "test-exec-iter-1");

        // Get the log
        let retrieved = manager.get_iteration_log("test-exec-iter-1").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.execution_id, "test-exec");
        assert_eq!(retrieved.iteration, 1);
        assert_eq!(retrieved.exit_code, 0);
        assert_eq!(retrieved.stdout, "All tests passed");

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_list_iteration_logs() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create an execution
        let exec = LoopExecution::with_id("list-test-exec", "ralph");
        manager.create_execution(exec).await.unwrap();

        // Create multiple iteration logs (out of order)
        let log3 = IterationLog::new("list-test-exec", 3).with_exit_code(0);
        let log1 = IterationLog::new("list-test-exec", 1).with_exit_code(1);
        let log2 = IterationLog::new("list-test-exec", 2).with_exit_code(1);

        manager.create_iteration_log(log3).await.unwrap();
        manager.create_iteration_log(log1).await.unwrap();
        manager.create_iteration_log(log2).await.unwrap();

        // List should return in iteration order (ascending)
        let logs = manager.list_iteration_logs("list-test-exec").await.unwrap();
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].iteration, 1);
        assert_eq!(logs[1].iteration, 2);
        assert_eq!(logs[2].iteration, 3);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_iteration_logs() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create an execution
        let exec = LoopExecution::with_id("delete-test-exec", "ralph");
        manager.create_execution(exec).await.unwrap();

        // Create iteration logs
        let log1 = IterationLog::new("delete-test-exec", 1);
        let log2 = IterationLog::new("delete-test-exec", 2);
        manager.create_iteration_log(log1).await.unwrap();
        manager.create_iteration_log(log2).await.unwrap();

        // Verify they exist
        let logs = manager.list_iteration_logs("delete-test-exec").await.unwrap();
        assert_eq!(logs.len(), 2);

        // Delete all iteration logs
        let deleted = manager.delete_iteration_logs("delete-test-exec").await.unwrap();
        assert_eq!(deleted, 2);

        // Verify they're gone
        let logs = manager.list_iteration_logs("delete-test-exec").await.unwrap();
        assert!(logs.is_empty());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_cascade_delete_execution_deletes_logs() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create an execution
        let exec = LoopExecution::with_id("cascade-test-exec", "ralph");
        manager.create_execution(exec).await.unwrap();

        // Create iteration logs
        let log1 = IterationLog::new("cascade-test-exec", 1);
        let log2 = IterationLog::new("cascade-test-exec", 2);
        manager.create_iteration_log(log1).await.unwrap();
        manager.create_iteration_log(log2).await.unwrap();

        // Delete the execution (should cascade delete logs)
        manager.delete_execution("cascade-test-exec").await.unwrap();

        // Verify logs are gone
        let logs = manager.list_iteration_logs("cascade-test-exec").await.unwrap();
        assert!(logs.is_empty());

        // Also verify the individual logs return None
        let log = manager.get_iteration_log("cascade-test-exec-iter-1").await.unwrap();
        assert!(log.is_none());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_iteration_log_created_event() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();
        let mut event_rx = manager.subscribe_events();

        // Create an execution
        let exec = LoopExecution::with_id("event-test-exec", "ralph");
        manager.create_execution(exec).await.unwrap();
        // Drain the ExecutionCreated and possibly ExecutionPending events
        while event_rx.try_recv().is_ok() {}

        // Create an iteration log
        let log = IterationLog::new("event-test-exec", 1).with_exit_code(0);
        manager.create_iteration_log(log).await.unwrap();

        // Should receive IterationLogCreated event
        let event = event_rx.try_recv().unwrap();
        match event {
            StateEvent::IterationLogCreated {
                execution_id,
                iteration,
                exit_code,
            } => {
                assert_eq!(execution_id, "event-test-exec");
                assert_eq!(iteration, 1);
                assert_eq!(exit_code, 0);
            }
            _ => panic!("Expected IterationLogCreated event, got {:?}", event),
        }

        manager.shutdown().await.unwrap();
    }
}
