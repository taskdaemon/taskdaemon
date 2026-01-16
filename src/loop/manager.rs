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

use crate::coordinator::{CoordRequest, CoordinatorHandle};
use crate::domain::{Loop, LoopExecution, LoopExecutionStatus};
use crate::llm::LlmClient;
use crate::r#loop::{LoopConfig, LoopEngine};
use crate::scheduler::Scheduler;
use crate::state::StateManager;
use crate::worktree::{MergeResult, WorktreeConfig, WorktreeManager, merge_to_main};

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

    /// Coordinator sender for registration and control
    coordinator_tx: mpsc::Sender<CoordRequest>,

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
    ///
    /// Takes a coordinator sender (not the Coordinator itself) because the
    /// Coordinator runs as its own task. This allows proper ownership:
    /// - Coordinator::run() consumes self
    /// - LoopManager communicates via the sender
    pub fn new(
        config: LoopManagerConfig,
        coordinator_tx: mpsc::Sender<CoordRequest>,
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
            coordinator_tx,
            scheduler: Arc::new(scheduler),
            llm,
            state,
            worktree_manager: WorktreeManager::new(worktree_config),
            loop_configs,
            shutdown_requested: false,
        }
    }

    /// Create a CoordinatorHandle for a new execution by registering with the Coordinator
    ///
    /// This sends a Register message to the Coordinator and creates a handle with
    /// its own message receiver channel.
    async fn create_coord_handle(&self, exec_id: &str) -> Result<CoordinatorHandle> {
        let (msg_tx, msg_rx) = mpsc::channel(100);

        self.coordinator_tx
            .send(CoordRequest::Register {
                exec_id: exec_id.to_string(),
                tx: msg_tx,
            })
            .await
            .map_err(|_| eyre::eyre!("Coordinator channel closed"))?;

        Ok(CoordinatorHandle::new(
            self.coordinator_tx.clone(),
            msg_rx,
            exec_id.to_string(),
        ))
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
    ///
    /// The cascade system creates LoopExecution records when parent loops complete.
    /// This method finds pending executions with satisfied dependencies and spawns them.
    async fn poll_and_spawn(&mut self) -> Result<()> {
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

    /// Spawn a loop execution as a tokio task
    pub async fn spawn_loop(&mut self, exec: &LoopExecution) -> Result<()> {
        // Check if already running
        if self.tasks.contains_key(&exec.id) {
            debug!(exec_id = %exec.id, "Loop already running");
            return Ok(());
        }

        // Wait for scheduler slot (handles rate limiting and priority queuing)
        // TODO: Extract priority from parent Spec/Plan once we wire that up
        self.scheduler
            .wait_for_slot(&exec.id, crate::domain::Priority::Normal)
            .await
            .context("Failed to acquire scheduler slot")?;

        // Create/verify worktree
        let worktree_info = self
            .worktree_manager
            .create(&exec.id)
            .await
            .context("Failed to create worktree")?;

        // Get loop config for this type
        let loop_config = self.loop_configs.get(&exec.loop_type).cloned().unwrap_or_default();

        // Register with coordinator and get a handle
        let coord_handle = self
            .create_coord_handle(&exec.id)
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
        let repo_root = self.config.repo_root.clone();
        let scheduler = self.scheduler.clone();

        let handle = tokio::spawn(async move {
            // Build engine with coordinator and scheduler for rate limiting
            let engine =
                LoopEngine::with_coordinator(exec_id.clone(), loop_config, llm, worktree_path.clone(), coord_handle)
                    .with_scheduler(scheduler.clone());

            let result = run_loop_task(engine, state, worktree_path, repo_root).await;

            // Mark scheduler slot as complete (releases slot for next queued request)
            scheduler.complete(&exec_id).await;

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
                .coordinator_tx
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

        // Shutdown coordinator by sending shutdown message
        let _ = self.coordinator_tx.send(CoordRequest::Shutdown).await;

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
        self.coordinator_tx
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
///
/// On successful completion, merges the worktree branch to main.
async fn run_loop_task(
    mut engine: LoopEngine,
    state: StateManager,
    worktree_path: PathBuf,
    repo_root: PathBuf,
) -> LoopTaskResult {
    let exec_id = engine.exec_id.clone();

    match engine.run().await {
        Ok(crate::r#loop::IterationResult::Complete { iterations }) => {
            // Get spec title for merge commit message
            let spec_title = state
                .get_execution(&exec_id)
                .await
                .ok()
                .flatten()
                .and_then(|e| e.context.get("title").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_else(|| "Completed work".to_string());

            // Merge to main before marking complete
            match merge_to_main(&repo_root, &worktree_path, &exec_id, &spec_title).await {
                Ok(MergeResult::Success) => {
                    info!(exec_id = %exec_id, "Successfully merged to main");
                    // Update state to complete with progress
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Complete);
                        exec.iteration = engine.current_iteration();
                        exec.progress = engine.get_progress();
                        let _ = state.update_execution(exec).await;
                    }
                    LoopTaskResult::Complete { exec_id, iterations }
                }
                Ok(MergeResult::Conflict { message }) => {
                    warn!(exec_id = %exec_id, "Merge conflict: {}", message);
                    // Mark as blocked - needs manual intervention
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Blocked);
                        exec.set_error(format!("Merge conflict: {}", message));
                        exec.iteration = engine.current_iteration();
                        exec.progress = engine.get_progress();
                        let _ = state.update_execution(exec).await;
                    }
                    LoopTaskResult::Failed {
                        exec_id,
                        reason: format!("Merge conflict: {}", message),
                    }
                }
                Ok(MergeResult::PushFailed { message }) => {
                    warn!(exec_id = %exec_id, "Push failed: {}", message);
                    // Mark as failed - push issue
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Failed);
                        exec.set_error(format!("Push failed: {}", message));
                        exec.iteration = engine.current_iteration();
                        exec.progress = engine.get_progress();
                        let _ = state.update_execution(exec).await;
                    }
                    LoopTaskResult::Failed {
                        exec_id,
                        reason: format!("Push failed: {}", message),
                    }
                }
                Err(e) => {
                    error!(exec_id = %exec_id, "Merge error: {}", e);
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Failed);
                        exec.set_error(format!("Merge error: {}", e));
                        exec.iteration = engine.current_iteration();
                        exec.progress = engine.get_progress();
                        let _ = state.update_execution(exec).await;
                    }
                    LoopTaskResult::Failed {
                        exec_id,
                        reason: format!("Merge error: {}", e),
                    }
                }
            }
        }
        Ok(crate::r#loop::IterationResult::Interrupted { reason: _ }) => {
            // Update state to stopped with progress
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Stopped);
                exec.iteration = engine.current_iteration();
                exec.progress = engine.get_progress();
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Stopped { exec_id }
        }
        Ok(crate::r#loop::IterationResult::Error { message, .. }) => {
            // Update state to failed with progress
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_error(&message);
                exec.iteration = engine.current_iteration();
                exec.progress = engine.get_progress();
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
                exec.iteration = engine.current_iteration();
                exec.progress = engine.get_progress();
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Failed {
                exec_id,
                reason: "Unexpected loop result".to_string(),
            }
        }
        Err(e) => {
            // Update state to failed with progress
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_error(e.to_string());
                exec.iteration = engine.current_iteration();
                exec.progress = engine.get_progress();
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
pub fn validate_dependency_graph<'a>(loops: impl IntoIterator<Item = &'a Loop>) -> Result<(), Vec<String>> {
    let loop_map: HashMap<&str, &Loop> = loops.into_iter().map(|l| (l.id.as_str(), l)).collect();

    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut cycle_path = Vec::new();

    for loop_id in loop_map.keys() {
        if !visited.contains(loop_id)
            && has_cycle_dfs(loop_id, &loop_map, &mut visited, &mut rec_stack, &mut cycle_path)
        {
            return Err(cycle_path);
        }
    }

    Ok(())
}

/// DFS helper for cycle detection
fn has_cycle_dfs<'a>(
    node: &'a str,
    graph: &HashMap<&'a str, &'a Loop>,
    visited: &mut HashSet<&'a str>,
    rec_stack: &mut HashSet<&'a str>,
    cycle_path: &mut Vec<String>,
) -> bool {
    visited.insert(node);
    rec_stack.insert(node);
    cycle_path.push(node.to_string());

    if let Some(record) = graph.get(node) {
        for dep_id in &record.deps {
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

/// Topologically sort loops by dependencies
///
/// Returns loops in execution order (dependencies first).
/// The returned Vec contains indices into the input slice.
pub fn topological_sort(loops: &[Loop]) -> Result<Vec<usize>, Vec<String>> {
    // First validate no cycles
    validate_dependency_graph(loops)?;

    // Build index map: id -> index in loops
    let index_map: HashMap<&str, usize> = loops.iter().enumerate().map(|(i, l)| (l.id.as_str(), i)).collect();

    let mut visited = HashSet::new();
    let mut result = Vec::new();

    for idx in 0..loops.len() {
        topo_dfs_idx(idx, loops, &index_map, &mut visited, &mut result);
    }

    Ok(result)
}

/// DFS helper for topological sort (returns indices)
fn topo_dfs_idx(
    idx: usize,
    loops: &[Loop],
    index_map: &HashMap<&str, usize>,
    visited: &mut HashSet<usize>,
    result: &mut Vec<usize>,
) {
    if visited.contains(&idx) {
        return;
    }

    visited.insert(idx);

    let record = &loops[idx];
    for dep_id in &record.deps {
        if let Some(&dep_idx) = index_map.get(dep_id.as_str()) {
            topo_dfs_idx(dep_idx, loops, index_map, visited, result);
        }
    }
    result.push(idx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_detection_no_cycle() {
        let loops = vec![
            Loop::with_id("loop-1", "mytype", "Loop 1"),
            {
                let mut l = Loop::with_id("loop-2", "mytype", "Loop 2");
                l.deps = vec!["loop-1".to_string()];
                l
            },
            {
                let mut l = Loop::with_id("loop-3", "mytype", "Loop 3");
                l.deps = vec!["loop-1".to_string(), "loop-2".to_string()];
                l
            },
        ];

        let result = validate_dependency_graph(&loops);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cycle_detection_with_cycle() {
        let loops = vec![
            {
                let mut l = Loop::with_id("loop-1", "mytype", "Loop 1");
                l.deps = vec!["loop-3".to_string()];
                l
            },
            {
                let mut l = Loop::with_id("loop-2", "mytype", "Loop 2");
                l.deps = vec!["loop-1".to_string()];
                l
            },
            {
                let mut l = Loop::with_id("loop-3", "mytype", "Loop 3");
                l.deps = vec!["loop-2".to_string()];
                l
            },
        ];

        let result = validate_dependency_graph(&loops);
        assert!(result.is_err());
    }

    #[test]
    fn test_cycle_detection_self_cycle() {
        let loops = vec![{
            let mut l = Loop::with_id("loop-1", "mytype", "Loop 1");
            l.deps = vec!["loop-1".to_string()];
            l
        }];

        let result = validate_dependency_graph(&loops);
        assert!(result.is_err());
    }

    #[test]
    fn test_topological_sort_simple() {
        let loops = vec![
            Loop::with_id("loop-1", "mytype", "Loop 1"),
            {
                let mut l = Loop::with_id("loop-2", "mytype", "Loop 2");
                l.deps = vec!["loop-1".to_string()];
                l
            },
            {
                let mut l = Loop::with_id("loop-3", "mytype", "Loop 3");
                l.deps = vec!["loop-2".to_string()];
                l
            },
        ];

        let sorted_indices = topological_sort(&loops).unwrap();

        // loop-1 (index 0) should come before loop-2 (index 1), loop-2 before loop-3 (index 2)
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
        let loops = vec![
            Loop::with_id("A", "mytype", "A"),
            {
                let mut l = Loop::with_id("B", "mytype", "B");
                l.deps = vec!["A".to_string()];
                l
            },
            {
                let mut l = Loop::with_id("C", "mytype", "C");
                l.deps = vec!["A".to_string()];
                l
            },
            {
                let mut l = Loop::with_id("D", "mytype", "D");
                l.deps = vec!["B".to_string(), "C".to_string()];
                l
            },
        ];

        let sorted_indices = topological_sort(&loops).unwrap();

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
        let loops: Vec<Loop> = vec![];
        let sorted = topological_sort(&loops).unwrap();
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_topological_sort_no_deps() {
        let loops = vec![
            Loop::with_id("loop-1", "mytype", "Loop 1"),
            Loop::with_id("loop-2", "mytype", "Loop 2"),
            Loop::with_id("loop-3", "mytype", "Loop 3"),
        ];

        let sorted = topological_sort(&loops).unwrap();
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
