//! LoopManager - top-level orchestrator for spawning and managing loops
//!
//! The LoopManager is responsible for:
//! - Spawning loops as tokio tasks
//! - Tracking loop lifecycle via task registry
//! - Resolving dependencies before spawning
//! - Enforcing concurrency limits via semaphore
//! - Graceful shutdown coordination

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eyre::{Context, Result};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::coordinator::{CoordRequest, Coordinator};
use crate::domain::{LoopExecution, LoopExecutionStatus, Spec, SpecStatus};
use crate::llm::LlmClient;
use crate::r#loop::{LoopConfig, LoopEngine};
use crate::scheduler::Scheduler;
use crate::state::StateManager;
use crate::worktree::{WorktreeConfig, WorktreeManager};

/// Configuration for the LoopManager
#[derive(Debug, Clone)]
pub struct LoopManagerConfig {
    /// Maximum concurrent loops
    pub max_concurrent_loops: usize,

    /// Polling interval for ready loops (in seconds)
    pub poll_interval_secs: u64,

    /// Shutdown timeout (in seconds)
    pub shutdown_timeout_secs: u64,

    /// Repository root path
    pub repo_root: PathBuf,

    /// Worktree base directory
    pub worktree_dir: PathBuf,
}

impl Default for LoopManagerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_loops: 50,
            poll_interval_secs: 10,
            shutdown_timeout_secs: 60,
            repo_root: PathBuf::from("."),
            worktree_dir: PathBuf::from("/tmp/taskdaemon/worktrees"),
        }
    }
}

/// Result of a loop task
#[derive(Debug)]
pub enum LoopTaskResult {
    /// Loop completed successfully
    Complete { exec_id: String, iterations: u32 },
    /// Loop failed
    Failed { exec_id: String, reason: String },
    /// Loop was stopped
    Stopped { exec_id: String },
}

/// LoopManager orchestrates the spawning and lifecycle of loops
pub struct LoopManager {
    /// Configuration
    config: LoopManagerConfig,

    /// Running loop tasks by exec_id
    tasks: HashMap<String, JoinHandle<LoopTaskResult>>,

    /// Concurrency limiter
    semaphore: Arc<Semaphore>,

    /// Shared coordinator for inter-loop communication
    coordinator: Arc<Coordinator>,

    /// Scheduler for API rate limiting
    scheduler: Arc<Scheduler>,

    /// LLM client
    llm: Arc<dyn LlmClient>,

    /// State manager
    state: StateManager,

    /// Worktree manager
    worktree_manager: WorktreeManager,

    /// Loop configurations by type
    loop_configs: HashMap<String, LoopConfig>,

    /// Shutdown flag
    shutdown_requested: bool,
}

impl LoopManager {
    /// Create a new LoopManager
    pub fn new(
        config: LoopManagerConfig,
        coordinator: Coordinator,
        scheduler: Scheduler,
        llm: Arc<dyn LlmClient>,
        state: StateManager,
        loop_configs: HashMap<String, LoopConfig>,
    ) -> Self {
        let worktree_config = WorktreeConfig {
            base_dir: config.worktree_dir.clone(),
            repo_root: config.repo_root.clone(),
            min_disk_space_gb: 5,
            branch_prefix: "taskdaemon".to_string(),
        };

        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_loops)),
            config,
            tasks: HashMap::new(),
            coordinator: Arc::new(coordinator),
            scheduler: Arc::new(scheduler),
            llm,
            state,
            worktree_manager: WorktreeManager::new(worktree_config),
            loop_configs,
            shutdown_requested: false,
        }
    }

    /// Run the manager's main loop
    ///
    /// This polls for ready loops and spawns them, handling shutdown gracefully.
    pub async fn run(&mut self, mut shutdown_rx: mpsc::Receiver<()>) -> Result<()> {
        info!("LoopManager starting");

        // Run recovery first
        self.recover_interrupted_loops().await?;

        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = tokio::time::interval(poll_interval);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if !self.shutdown_requested {
                        self.poll_and_spawn().await?;
                    }
                    self.reap_completed_tasks().await;
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received");
                    self.shutdown_requested = true;
                    break;
                }
            }
        }

        // Graceful shutdown
        self.shutdown().await?;

        Ok(())
    }

    /// Poll for ready loops and spawn them
    async fn poll_and_spawn(&mut self) -> Result<()> {
        // Find pending Specs with satisfied dependencies
        let pending_specs = self
            .state
            .list_specs(None, Some("pending".to_string()))
            .await
            .context("Failed to list pending specs")?;

        for spec in pending_specs {
            if self.dependencies_satisfied(&spec).await? {
                self.spawn_spec_loop(&spec).await?;
            }
        }

        // Find pending LoopExecutions with satisfied dependencies
        let pending_executions = self
            .state
            .list_executions(Some("pending".to_string()), None)
            .await
            .context("Failed to list pending executions")?;

        for exec in pending_executions {
            if self.loop_deps_satisfied(&exec).await? {
                self.spawn_loop(&exec).await?;
            }
        }

        Ok(())
    }

    /// Check if a Spec's dependencies are satisfied
    async fn dependencies_satisfied(&self, spec: &Spec) -> Result<bool> {
        for dep_id in &spec.deps {
            if let Some(dep_spec) = self.state.get_spec(dep_id).await? {
                if dep_spec.status != SpecStatus::Complete {
                    return Ok(false);
                }
            } else {
                // Dependency not found - can't proceed
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Check if a LoopExecution's dependencies are satisfied
    async fn loop_deps_satisfied(&self, exec: &LoopExecution) -> Result<bool> {
        for dep_id in &exec.deps {
            if let Some(dep_exec) = self.state.get_execution(dep_id).await? {
                if dep_exec.status != LoopExecutionStatus::Complete {
                    return Ok(false);
                }
            } else {
                // Dependency not found - can't proceed
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Spawn a loop for a Spec
    async fn spawn_spec_loop(&mut self, spec: &Spec) -> Result<()> {
        // Create LoopExecution record
        let mut exec = LoopExecution::new("spec", &spec.id);
        exec.set_parent(&spec.id);

        // Set context from spec
        exec.set_context(serde_json::json!({
            "spec_id": spec.id,
            "title": spec.title,
            "spec_file": spec.file,
        }));

        // Store the execution
        self.state
            .create_execution(exec.clone())
            .await
            .context("Failed to create execution record")?;

        // Spawn the loop
        self.spawn_loop(&exec).await
    }

    /// Spawn a loop execution as a tokio task
    pub async fn spawn_loop(&mut self, exec: &LoopExecution) -> Result<()> {
        // Check if already running
        if self.tasks.contains_key(&exec.id) {
            debug!(exec_id = %exec.id, "Loop already running");
            return Ok(());
        }

        // Acquire semaphore permit
        let permit = self.semaphore.clone().acquire_owned().await?;

        // Create/verify worktree
        let worktree_info = self
            .worktree_manager
            .create(&exec.id)
            .await
            .context("Failed to create worktree")?;

        // Get loop config for this type
        let loop_config = self.loop_configs.get(&exec.loop_type).cloned().unwrap_or_default();

        // Register with coordinator
        let coord_handle = self
            .coordinator
            .register(&exec.id)
            .await
            .context("Failed to register with coordinator")?;

        // Update execution status to running
        let mut exec_running = exec.clone();
        exec_running.set_status(LoopExecutionStatus::Running);
        exec_running.set_worktree(worktree_info.path.display().to_string());
        self.state.update_execution(exec_running).await?;

        // Build and spawn engine
        let exec_id = exec.id.clone();
        let llm = self.llm.clone();
        let state = self.state.clone();
        let worktree_path = worktree_info.path.clone();

        let handle = tokio::spawn(async move {
            let engine = LoopEngine::new(exec_id.clone(), loop_config, llm, worktree_path);

            let result = run_loop_task(engine, coord_handle, state).await;

            // Release permit when done
            drop(permit);

            result
        });

        self.tasks.insert(exec.id.clone(), handle);
        info!(exec_id = %exec.id, "Spawned loop");

        Ok(())
    }

    /// Reap completed tasks and update state
    async fn reap_completed_tasks(&mut self) {
        let mut completed_ids = Vec::new();

        for (exec_id, handle) in &self.tasks {
            if handle.is_finished() {
                completed_ids.push(exec_id.clone());
            }
        }

        for exec_id in completed_ids {
            if let Some(handle) = self.tasks.remove(&exec_id) {
                match handle.await {
                    Ok(LoopTaskResult::Complete { exec_id, iterations }) => {
                        info!(exec_id = %exec_id, iterations, "Loop completed successfully");
                    }
                    Ok(LoopTaskResult::Failed { exec_id, reason }) => {
                        error!(exec_id = %exec_id, reason = %reason, "Loop failed");
                    }
                    Ok(LoopTaskResult::Stopped { exec_id }) => {
                        info!(exec_id = %exec_id, "Loop stopped");
                    }
                    Err(e) => {
                        error!(exec_id = %exec_id, error = %e, "Loop task panicked");
                    }
                }

                // Cleanup worktree
                if let Err(e) = self.worktree_manager.remove(&exec_id).await {
                    warn!(exec_id = %exec_id, error = %e, "Failed to remove worktree");
                }
            }
        }
    }

    /// Recover interrupted loops on startup
    async fn recover_interrupted_loops(&mut self) -> Result<()> {
        info!("Scanning for interrupted loops to recover");

        let recoverable_statuses = ["running", "rebasing", "paused"];

        for status in recoverable_statuses {
            let executions = self.state.list_executions(Some(status.to_string()), None).await?;

            for mut exec in executions {
                info!(exec_id = %exec.id, status = %status, "Found interrupted loop");

                // Check if worktree exists
                if self.worktree_manager.exists(&exec.id) {
                    // Resume from current state
                    exec.set_status(LoopExecutionStatus::Pending);
                    self.state.update_execution(exec).await?;
                } else {
                    // Mark as failed - can't recover without worktree
                    exec.set_status(LoopExecutionStatus::Failed);
                    exec.set_error("Worktree missing during recovery");
                    self.state.update_execution(exec).await?;
                }
            }
        }

        Ok(())
    }

    /// Gracefully shutdown all running loops
    async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down LoopManager with {} active loops", self.tasks.len());

        // Request stop for all running loops via coordinator
        for exec_id in self.tasks.keys() {
            let _ = self
                .coordinator
                .sender()
                .send(CoordRequest::Stop {
                    from_exec_id: "loop_manager".to_string(),
                    target_exec_id: exec_id.clone(),
                    reason: "Daemon shutdown".to_string(),
                })
                .await;
        }

        // Wait for tasks with timeout
        let timeout = Duration::from_secs(self.config.shutdown_timeout_secs);
        let deadline = tokio::time::Instant::now() + timeout;

        while !self.tasks.is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.reap_completed_tasks().await;
        }

        // Force abort remaining tasks
        if !self.tasks.is_empty() {
            warn!("Aborting {} remaining loops after timeout", self.tasks.len());
            for (exec_id, handle) in self.tasks.drain() {
                handle.abort();

                // Mark as stopped in state
                if let Ok(Some(mut exec)) = self.state.get_execution(&exec_id).await {
                    exec.set_status(LoopExecutionStatus::Stopped);
                    let _ = self.state.update_execution(exec).await;
                }
            }
        }

        // Shutdown coordinator
        self.coordinator.shutdown().await?;

        info!("LoopManager shutdown complete");
        Ok(())
    }

    /// Get the number of running loops
    pub fn running_count(&self) -> usize {
        self.tasks.len()
    }

    /// Get IDs of all running loops
    pub fn running_ids(&self) -> Vec<String> {
        self.tasks.keys().cloned().collect()
    }

    /// Stop a specific loop
    pub async fn stop_loop(&self, exec_id: &str) -> Result<()> {
        self.coordinator
            .sender()
            .send(CoordRequest::Stop {
                from_exec_id: "loop_manager".to_string(),
                target_exec_id: exec_id.to_string(),
                reason: "User requested stop".to_string(),
            })
            .await
            .map_err(|_| eyre::eyre!("Coordinator channel closed"))?;

        Ok(())
    }
}

/// Run a loop task and handle completion
async fn run_loop_task(
    mut engine: LoopEngine,
    _coord_handle: crate::coordinator::CoordinatorHandle,
    state: StateManager,
) -> LoopTaskResult {
    let exec_id = engine.exec_id.clone();

    match engine.run().await {
        Ok(crate::r#loop::IterationResult::Complete { iterations }) => {
            // Update state to complete
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Complete);
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Complete { exec_id, iterations }
        }
        Ok(crate::r#loop::IterationResult::Interrupted { reason: _ }) => {
            // Update state to stopped
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Stopped);
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Stopped { exec_id }
        }
        Ok(crate::r#loop::IterationResult::Error { message, .. }) => {
            // Update state to failed
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_error(&message);
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Failed {
                exec_id,
                reason: message,
            }
        }
        Ok(_) => {
            // Other results treated as failure
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_error("Unexpected loop result");
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Failed {
                exec_id,
                reason: "Unexpected loop result".to_string(),
            }
        }
        Err(e) => {
            // Update state to failed
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_error(e.to_string());
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Failed {
                exec_id,
                reason: e.to_string(),
            }
        }
    }
}

/// Validate a dependency graph for cycles
///
/// Uses DFS to detect cycles. Returns Ok(()) if no cycles, Err with cycle info if found.
pub fn validate_dependency_graph<'a>(specs: impl IntoIterator<Item = &'a Spec>) -> Result<(), Vec<String>> {
    let spec_map: HashMap<&str, &Spec> = specs.into_iter().map(|s| (s.id.as_str(), s)).collect();

    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut cycle_path = Vec::new();

    for spec_id in spec_map.keys() {
        if !visited.contains(spec_id)
            && has_cycle_dfs(spec_id, &spec_map, &mut visited, &mut rec_stack, &mut cycle_path)
        {
            return Err(cycle_path);
        }
    }

    Ok(())
}

/// DFS helper for cycle detection
fn has_cycle_dfs<'a>(
    node: &'a str,
    graph: &HashMap<&'a str, &'a Spec>,
    visited: &mut HashSet<&'a str>,
    rec_stack: &mut HashSet<&'a str>,
    cycle_path: &mut Vec<String>,
) -> bool {
    visited.insert(node);
    rec_stack.insert(node);
    cycle_path.push(node.to_string());

    if let Some(spec) = graph.get(node) {
        for dep_id in &spec.deps {
            if !visited.contains(dep_id.as_str()) {
                if graph.contains_key(dep_id.as_str())
                    && has_cycle_dfs(dep_id.as_str(), graph, visited, rec_stack, cycle_path)
                {
                    return true;
                }
            } else if rec_stack.contains(dep_id.as_str()) {
                cycle_path.push(dep_id.clone());
                return true;
            }
        }
    }

    rec_stack.remove(node);
    cycle_path.pop();
    false
}

/// Topologically sort specs by dependencies
///
/// Returns specs in execution order (dependencies first).
/// The returned Vec contains indices into the input slice.
pub fn topological_sort(specs: &[Spec]) -> Result<Vec<usize>, Vec<String>> {
    // First validate no cycles
    validate_dependency_graph(specs)?;

    // Build index map: id -> index in specs
    let index_map: HashMap<&str, usize> = specs.iter().enumerate().map(|(i, s)| (s.id.as_str(), i)).collect();

    let mut visited = HashSet::new();
    let mut result = Vec::new();

    for idx in 0..specs.len() {
        topo_dfs_idx(idx, specs, &index_map, &mut visited, &mut result);
    }

    Ok(result)
}

/// DFS helper for topological sort (returns indices)
fn topo_dfs_idx(
    idx: usize,
    specs: &[Spec],
    index_map: &HashMap<&str, usize>,
    visited: &mut HashSet<usize>,
    result: &mut Vec<usize>,
) {
    if visited.contains(&idx) {
        return;
    }

    visited.insert(idx);

    let spec = &specs[idx];
    for dep_id in &spec.deps {
        if let Some(&dep_idx) = index_map.get(dep_id.as_str()) {
            topo_dfs_idx(dep_idx, specs, index_map, visited, result);
        }
    }
    result.push(idx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_detection_no_cycle() {
        let specs = vec![
            Spec::with_id("spec-1", "plan-1", "Spec 1", "/spec1.md"),
            {
                let mut s = Spec::with_id("spec-2", "plan-1", "Spec 2", "/spec2.md");
                s.deps = vec!["spec-1".to_string()];
                s
            },
            {
                let mut s = Spec::with_id("spec-3", "plan-1", "Spec 3", "/spec3.md");
                s.deps = vec!["spec-1".to_string(), "spec-2".to_string()];
                s
            },
        ];

        let result = validate_dependency_graph(&specs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cycle_detection_with_cycle() {
        let specs = vec![
            {
                let mut s = Spec::with_id("spec-1", "plan-1", "Spec 1", "/spec1.md");
                s.deps = vec!["spec-3".to_string()];
                s
            },
            {
                let mut s = Spec::with_id("spec-2", "plan-1", "Spec 2", "/spec2.md");
                s.deps = vec!["spec-1".to_string()];
                s
            },
            {
                let mut s = Spec::with_id("spec-3", "plan-1", "Spec 3", "/spec3.md");
                s.deps = vec!["spec-2".to_string()];
                s
            },
        ];

        let result = validate_dependency_graph(&specs);
        assert!(result.is_err());
    }

    #[test]
    fn test_cycle_detection_self_cycle() {
        let specs = vec![{
            let mut s = Spec::with_id("spec-1", "plan-1", "Spec 1", "/spec1.md");
            s.deps = vec!["spec-1".to_string()];
            s
        }];

        let result = validate_dependency_graph(&specs);
        assert!(result.is_err());
    }

    #[test]
    fn test_topological_sort_simple() {
        let specs = vec![
            Spec::with_id("spec-1", "plan-1", "Spec 1", "/spec1.md"),
            {
                let mut s = Spec::with_id("spec-2", "plan-1", "Spec 2", "/spec2.md");
                s.deps = vec!["spec-1".to_string()];
                s
            },
            {
                let mut s = Spec::with_id("spec-3", "plan-1", "Spec 3", "/spec3.md");
                s.deps = vec!["spec-2".to_string()];
                s
            },
        ];

        let sorted_indices = topological_sort(&specs).unwrap();

        // spec-1 (index 0) should come before spec-2 (index 1), spec-2 before spec-3 (index 2)
        let pos_1 = sorted_indices.iter().position(|&i| i == 0).unwrap();
        let pos_2 = sorted_indices.iter().position(|&i| i == 1).unwrap();
        let pos_3 = sorted_indices.iter().position(|&i| i == 2).unwrap();

        assert!(pos_1 < pos_2);
        assert!(pos_2 < pos_3);
    }

    #[test]
    fn test_topological_sort_diamond() {
        // Diamond dependency: A <- B, A <- C, B <- D, C <- D
        // A = index 0, B = index 1, C = index 2, D = index 3
        let specs = vec![
            Spec::with_id("A", "plan-1", "A", "/a.md"),
            {
                let mut s = Spec::with_id("B", "plan-1", "B", "/b.md");
                s.deps = vec!["A".to_string()];
                s
            },
            {
                let mut s = Spec::with_id("C", "plan-1", "C", "/c.md");
                s.deps = vec!["A".to_string()];
                s
            },
            {
                let mut s = Spec::with_id("D", "plan-1", "D", "/d.md");
                s.deps = vec!["B".to_string(), "C".to_string()];
                s
            },
        ];

        let sorted_indices = topological_sort(&specs).unwrap();

        let pos_a = sorted_indices.iter().position(|&i| i == 0).unwrap(); // A
        let pos_b = sorted_indices.iter().position(|&i| i == 1).unwrap(); // B
        let pos_c = sorted_indices.iter().position(|&i| i == 2).unwrap(); // C
        let pos_d = sorted_indices.iter().position(|&i| i == 3).unwrap(); // D

        // A should come before B and C
        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        // B and C should come before D
        assert!(pos_b < pos_d);
        assert!(pos_c < pos_d);
    }

    #[test]
    fn test_topological_sort_empty() {
        let specs: Vec<Spec> = vec![];
        let sorted = topological_sort(&specs).unwrap();
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_topological_sort_no_deps() {
        let specs = vec![
            Spec::with_id("spec-1", "plan-1", "Spec 1", "/spec1.md"),
            Spec::with_id("spec-2", "plan-1", "Spec 2", "/spec2.md"),
            Spec::with_id("spec-3", "plan-1", "Spec 3", "/spec3.md"),
        ];

        let sorted = topological_sort(&specs).unwrap();
        assert_eq!(sorted.len(), 3);
    }

    #[test]
    fn test_loop_manager_config_default() {
        let config = LoopManagerConfig::default();
        assert_eq!(config.max_concurrent_loops, 50);
        assert_eq!(config.poll_interval_secs, 10);
        assert_eq!(config.shutdown_timeout_secs, 60);
    }
}
