//! StateManager - actor that owns TaskStore
//!
//! Processes commands via channels for thread-safe access to persistent state.

use std::path::Path;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::domain::{Filter, FilterOp, IndexValue, LoopExecution, LoopExecutionStatus, Plan, Spec, Store};

use super::messages::{StateCommand, StateError, StateResponse};

/// Aggregated metrics from the daemon's state
#[derive(Debug, Default, serde::Serialize)]
pub struct DaemonMetrics {
    /// Total number of loop executions
    pub total_executions: u64,
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

    /// Create a new Plan
    pub async fn create_plan(&self, plan: Plan) -> StateResponse<String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::CreatePlan { plan, reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Get a Plan by ID
    pub async fn get_plan(&self, id: &str) -> StateResponse<Option<Plan>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::GetPlan {
                id: id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Update a Plan
    pub async fn update_plan(&self, plan: Plan) -> StateResponse<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::UpdatePlan { plan, reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// List Plans with optional status filter
    pub async fn list_plans(&self, status_filter: Option<String>) -> StateResponse<Vec<Plan>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::ListPlans {
                status_filter,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Create a new Spec
    pub async fn create_spec(&self, spec: Spec) -> StateResponse<String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::CreateSpec { spec, reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Get a Spec by ID
    pub async fn get_spec(&self, id: &str) -> StateResponse<Option<Spec>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::GetSpec {
                id: id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// Update a Spec
    pub async fn update_spec(&self, spec: Spec) -> StateResponse<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::UpdateSpec { spec, reply: reply_tx })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

    /// List Specs with optional filters
    pub async fn list_specs(
        &self,
        parent_filter: Option<String>,
        status_filter: Option<String>,
    ) -> StateResponse<Vec<Spec>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StateCommand::ListSpecs {
                parent_filter,
                status_filter,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StateError::ChannelError)?;
        reply_rx.await.map_err(|_| StateError::ChannelError)?
    }

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

    // === Convenience methods for cascade logic ===

    /// List all Specs for a given Plan ID
    pub async fn list_specs_for_plan(&self, plan_id: &str) -> StateResponse<Vec<Spec>> {
        self.list_specs(Some(plan_id.to_string()), None).await
    }

    /// Get a Plan by ID, returning error if not found
    pub async fn get_plan_required(&self, id: &str) -> Result<Plan, StateError> {
        self.get_plan(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Plan {}", id)))
    }

    /// Get a Spec by ID, returning error if not found
    pub async fn get_spec_required(&self, id: &str) -> Result<Spec, StateError> {
        self.get_spec(id)
            .await?
            .ok_or_else(|| StateError::NotFound(format!("Spec {}", id)))
    }

    /// Get aggregated metrics from all loop executions
    pub async fn get_metrics(&self) -> eyre::Result<DaemonMetrics> {
        let executions = self.list_executions(None, None).await?;

        let mut metrics = DaemonMetrics::default();

        for exec in executions {
            metrics.total_executions += 1;
            match exec.status {
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

    /// Get a LoopExecution for a specific Spec (by parent field)
    pub async fn get_loop_execution_for_spec(&self, spec_id: &str) -> StateResponse<Option<LoopExecution>> {
        // List executions and find one with matching parent
        let executions = self.list_executions(None, Some("phase".to_string())).await?;
        Ok(executions.into_iter().find(|e| e.parent.as_deref() == Some(spec_id)))
    }
}

/// The actor loop that owns the Store and processes commands
async fn actor_loop(mut store: Store, mut rx: mpsc::Receiver<StateCommand>) {
    debug!("StateManager actor started");

    while let Some(cmd) = rx.recv().await {
        match cmd {
            StateCommand::CreatePlan { plan, reply } => {
                let result = store.create(plan).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::GetPlan { id, reply } => {
                let result: StateResponse<Option<Plan>> =
                    store.get(&id).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::UpdatePlan { plan, reply } => {
                let result = store.update(plan).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::ListPlans { status_filter, reply } => {
                let filters = status_filter
                    .map(|s| {
                        vec![Filter {
                            field: "status".to_string(),
                            op: FilterOp::Eq,
                            value: IndexValue::String(s),
                        }]
                    })
                    .unwrap_or_default();

                let result: StateResponse<Vec<Plan>> =
                    store.list(&filters).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::CreateSpec { spec, reply } => {
                let result = store.create(spec).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::GetSpec { id, reply } => {
                let result: StateResponse<Option<Spec>> =
                    store.get(&id).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::UpdateSpec { spec, reply } => {
                let result = store.update(spec).map_err(|e| StateError::StoreError(e.to_string()));
                let _ = reply.send(result);
            }

            StateCommand::ListSpecs {
                parent_filter,
                status_filter,
                reply,
            } => {
                let mut filters = Vec::new();
                if let Some(parent) = parent_filter {
                    filters.push(Filter {
                        field: "parent".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(parent),
                    });
                }
                if let Some(status) = status_filter {
                    filters.push(Filter {
                        field: "status".to_string(),
                        op: FilterOp::Eq,
                        value: IndexValue::String(status),
                    });
                }

                let result: StateResponse<Vec<Spec>> =
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
                if let Ok(c) = store.rebuild_indexes::<Plan>() {
                    count += c;
                }
                if let Ok(c) = store.rebuild_indexes::<Spec>() {
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
    async fn test_state_manager_plan_crud() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create
        let plan = Plan::with_id("test-plan", "Test Plan", "/test.md");
        let id = manager.create_plan(plan.clone()).await.unwrap();
        assert_eq!(id, "test-plan");

        // Get
        let retrieved = manager.get_plan("test-plan").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().title, "Test Plan");

        // Update
        let mut updated = plan.clone();
        updated.set_status(crate::domain::PlanStatus::Ready);
        manager.update_plan(updated).await.unwrap();

        let retrieved = manager.get_plan("test-plan").await.unwrap().unwrap();
        assert_eq!(retrieved.status, crate::domain::PlanStatus::Ready);

        // List
        let plans = manager.list_plans(None).await.unwrap();
        assert_eq!(plans.len(), 1);

        // Shutdown
        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_state_manager_spec_crud() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create parent plan first
        let plan = Plan::with_id("parent-plan", "Parent Plan", "/parent.md");
        manager.create_plan(plan).await.unwrap();

        // Create spec
        let spec = Spec::with_id("test-spec", "parent-plan", "Test Spec", "/spec.md");
        let id = manager.create_spec(spec).await.unwrap();
        assert_eq!(id, "test-spec");

        // Get
        let retrieved = manager.get_spec("test-spec").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().parent, "parent-plan");

        // List by parent
        let specs = manager.list_specs(Some("parent-plan".to_string()), None).await.unwrap();
        assert_eq!(specs.len(), 1);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_state_manager_execution_crud() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create execution
        let exec = LoopExecution::with_id("test-exec", "ralph");
        let id = manager.create_execution(exec).await.unwrap();
        assert_eq!(id, "test-exec");

        // Get
        let retrieved = manager.get_execution("test-exec").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().loop_type, "ralph");

        // List by type
        let execs = manager.list_executions(None, Some("ralph".to_string())).await.unwrap();
        assert_eq!(execs.len(), 1);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_state_manager_get_nonexistent() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        let result = manager.get_plan("nonexistent").await.unwrap();
        assert!(result.is_none());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_state_manager_list_with_filter() {
        let temp = tempdir().unwrap();
        let manager = StateManager::spawn(temp.path()).unwrap();

        // Create plans with different statuses
        let mut plan1 = Plan::with_id("plan-1", "Plan 1", "/plan1.md");
        plan1.set_status(crate::domain::PlanStatus::Draft);
        manager.create_plan(plan1).await.unwrap();

        let mut plan2 = Plan::with_id("plan-2", "Plan 2", "/plan2.md");
        plan2.set_status(crate::domain::PlanStatus::Ready);
        manager.create_plan(plan2).await.unwrap();

        // List with filter
        let draft_plans = manager.list_plans(Some("draft".to_string())).await.unwrap();
        assert_eq!(draft_plans.len(), 1);
        assert_eq!(draft_plans[0].id, "plan-1");

        manager.shutdown().await.unwrap();
    }
}
