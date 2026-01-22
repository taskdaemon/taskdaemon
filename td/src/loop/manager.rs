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
use std::sync::{Arc, RwLock};
use std::time::Duration;

use eyre::{Context, Result};
use tokio::net::UnixListener;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::coordinator::{CoordRequest, CoordinatorHandle};
use crate::daemon::VERSION;
use crate::domain::{Loop, LoopExecution, LoopExecutionStatus, LoopStatus};
use crate::ipc::{DaemonMessage, DaemonResponse, read_message, send_response};
use crate::llm::LlmClient;
use crate::r#loop::{CascadeHandler, LoopConfig, LoopEngine, LoopLoader};
use crate::scheduler::Scheduler;
use crate::state::{StateEvent, StateManager};
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
            // Increased from 10s to 60s since event-driven pickup handles immediate work.
            // Polling is now a fallback for edge cases, orphan recovery, and missed events.
            poll_interval_secs: 60,
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

    /// Loop type loader for cascade hierarchy
    type_loader: Arc<RwLock<LoopLoader>>,

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
        type_loader: Arc<RwLock<LoopLoader>>,
    ) -> Self {
        debug!(
            max_concurrent = config.max_concurrent_loops,
            poll_interval = config.poll_interval_secs,
            ?config.repo_root,
            ?config.worktree_dir,
            "LoopManager::new: called"
        );
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
            type_loader,
            shutdown_requested: false,
        }
    }

    /// Create a CoordinatorHandle for a new execution by registering with the Coordinator
    ///
    /// This sends a Register message to the Coordinator and creates a handle with
    /// its own message receiver channel.
    async fn create_coord_handle(&self, exec_id: &str) -> Result<CoordinatorHandle> {
        debug!(%exec_id, "create_coord_handle: called");
        let (msg_tx, msg_rx) = mpsc::channel(100);

        self.coordinator_tx
            .send(CoordRequest::Register {
                exec_id: exec_id.to_string(),
                tx: msg_tx,
            })
            .await
            .map_err(|_| eyre::eyre!("Coordinator channel closed"))?;

        debug!(%exec_id, "create_coord_handle: registered with coordinator");
        Ok(CoordinatorHandle::new(
            self.coordinator_tx.clone(),
            msg_rx,
            exec_id.to_string(),
        ))
    }

    /// Run the manager's main loop
    ///
    /// This polls for ready loops and spawns them, handling shutdown gracefully.
    /// Also subscribes to state events for immediate work pickup when executions
    /// become pending (instead of waiting for the next poll interval).
    ///
    /// If an IPC listener is provided, it will also handle cross-process messages
    /// from the TUI/CLI for immediate work pickup.
    pub async fn run(&mut self, mut shutdown_rx: mpsc::Receiver<()>, ipc_listener: Option<UnixListener>) -> Result<()> {
        debug!("run: called");
        info!("LoopManager starting");

        // Subscribe to state events for immediate work pickup
        let mut state_events = self.state.subscribe_events();

        // Run recovery first
        debug!("run: starting recovery");
        self.recover_interrupted_loops().await?;

        // Immediately poll for work on startup - don't wait for first interval
        info!("LoopManager: checking for pending/running executions on startup");
        self.poll_and_spawn().await?;

        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = tokio::time::interval(poll_interval);

        // Check if we have an IPC listener
        let has_ipc = ipc_listener.is_some();
        if has_ipc {
            info!("LoopManager: IPC listener enabled for cross-process wake-up");
        }

        // Keep the listener for the select loop
        let ipc_listener = ipc_listener;

        loop {
            // Handle with or without IPC listener using a macro-like approach
            // We need to handle both cases since accept() requires &self
            if let Some(ref listener) = ipc_listener {
                tokio::select! {
                    // Handle IPC connections for cross-process wake-up
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((mut stream, _addr)) => {
                                debug!("run: IPC connection accepted");
                                if let Err(e) = self.handle_ipc_connection(&mut stream).await {
                                    warn!(error = %e, "run: IPC connection error");
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "run: IPC accept error");
                            }
                        }
                    }

                    // Handle in-process state events for immediate work pickup
                    event = state_events.recv() => {
                        self.handle_state_event(event).await?;
                    }

                    // Fallback polling for edge cases and orphan recovery
                    _ = interval.tick() => {
                        self.handle_poll_tick().await?;
                    }

                    _ = shutdown_rx.recv() => {
                        debug!("run: shutdown signal received");
                        info!("Shutdown signal received");
                        self.shutdown_requested = true;
                        break;
                    }
                }
            } else {
                // No IPC listener - original behavior
                tokio::select! {
                    // Handle in-process state events for immediate work pickup
                    event = state_events.recv() => {
                        self.handle_state_event(event).await?;
                    }

                    // Fallback polling for edge cases and orphan recovery
                    _ = interval.tick() => {
                        self.handle_poll_tick().await?;
                    }

                    _ = shutdown_rx.recv() => {
                        debug!("run: shutdown signal received");
                        info!("Shutdown signal received");
                        self.shutdown_requested = true;
                        break;
                    }
                }
            }
        }

        // Graceful shutdown
        debug!("run: starting graceful shutdown");
        self.shutdown().await?;

        debug!("run: complete");
        Ok(())
    }

    /// Handle a state event from the in-process broadcast channel
    async fn handle_state_event(
        &mut self,
        event: Result<StateEvent, tokio::sync::broadcast::error::RecvError>,
    ) -> Result<()> {
        match event {
            Ok(StateEvent::ExecutionPending { id }) => {
                debug!(%id, "handle_state_event: received ExecutionPending");
                self.try_spawn_execution(&id).await;
            }
            Ok(_) => {
                // Ignore other events (ExecutionCreated, ExecutionUpdated)
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                warn!("handle_state_event: channel closed, falling back to polling only");
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                debug!(n, "handle_state_event: lagged behind, doing full poll");
                if !self.shutdown_requested {
                    self.poll_and_spawn().await?;
                }
            }
        }
        Ok(())
    }

    /// Handle the poll interval tick
    async fn handle_poll_tick(&mut self) -> Result<()> {
        debug!("handle_poll_tick: tick");
        if !self.shutdown_requested {
            debug!("handle_poll_tick: polling and spawning");
            self.poll_and_spawn().await?;
        } else {
            debug!("handle_poll_tick: shutdown requested, skipping poll");
        }
        self.reap_completed_tasks().await;
        Ok(())
    }

    /// Handle an IPC connection from TUI/CLI
    async fn handle_ipc_connection(&mut self, stream: &mut tokio::net::UnixStream) -> Result<()> {
        let msg = read_message(stream).await?;
        debug!(?msg, "handle_ipc_connection: received message");

        let response = match msg {
            DaemonMessage::ExecutionPending { id } => {
                debug!(%id, "handle_ipc_connection: ExecutionPending");
                self.try_spawn_execution(&id).await;
                DaemonResponse::Ok
            }
            DaemonMessage::ExecutionResumed { id } => {
                debug!(%id, "handle_ipc_connection: ExecutionResumed");
                self.try_spawn_execution(&id).await;
                DaemonResponse::Ok
            }
            DaemonMessage::Ping => {
                debug!("handle_ipc_connection: Ping");
                DaemonResponse::Pong {
                    version: VERSION.to_string(),
                }
            }
            DaemonMessage::Shutdown => {
                debug!("handle_ipc_connection: Shutdown");
                self.shutdown_requested = true;
                DaemonResponse::Ok
            }
        };

        send_response(stream, response).await?;
        Ok(())
    }

    /// Try to spawn an execution if it exists and deps are satisfied
    async fn try_spawn_execution(&mut self, id: &str) {
        if let Ok(Some(exec)) = self.state.get_execution(id).await {
            if self.loop_deps_satisfied(&exec).await.unwrap_or(false) {
                debug!(%id, "try_spawn_execution: deps satisfied, spawning");
                if let Err(e) = self.spawn_loop(&exec).await {
                    warn!(%id, error = %e, "try_spawn_execution: failed to spawn");
                }
            } else {
                debug!(%id, "try_spawn_execution: deps not satisfied, will pick up on next poll");
            }
        } else {
            debug!(%id, "try_spawn_execution: execution not found");
        }
    }

    /// Poll for ready loops and spawn them
    ///
    /// The cascade system creates LoopExecution records when parent loops complete.
    /// This method finds pending/running executions with satisfied dependencies and spawns them.
    /// Also recovers "running" executions that aren't actually being processed (e.g., after restart).
    async fn poll_and_spawn(&mut self) -> Result<()> {
        debug!("poll_and_spawn: called");

        // Find pending LoopExecutions with satisfied dependencies
        let pending_executions = self
            .state
            .list_executions(Some("pending".to_string()), None)
            .await
            .context("Failed to list pending executions")?;
        debug!(
            pending_count = pending_executions.len(),
            "poll_and_spawn: found pending executions"
        );

        for exec in pending_executions {
            debug!(exec_id = %exec.id, "poll_and_spawn: checking deps for execution");
            if self.loop_deps_satisfied(&exec).await? {
                debug!(exec_id = %exec.id, "poll_and_spawn: deps satisfied, spawning");
                self.spawn_loop(&exec).await?;
            } else {
                debug!(exec_id = %exec.id, "poll_and_spawn: deps not satisfied");
            }
        }

        // Also find "running" executions that aren't actually being processed
        // This handles recovery after daemon restart or orphaned executions
        let running_executions = self
            .state
            .list_executions(Some("running".to_string()), None)
            .await
            .context("Failed to list running executions")?;

        for exec in running_executions {
            if !self.tasks.contains_key(&exec.id) {
                info!(exec_id = %exec.id, loop_type = %exec.loop_type, "poll_and_spawn: recovering orphaned running execution");
                self.spawn_loop(&exec).await?;
            }
        }

        debug!("poll_and_spawn: complete");
        Ok(())
    }

    /// Check if a LoopExecution's dependencies are satisfied
    async fn loop_deps_satisfied(&self, exec: &LoopExecution) -> Result<bool> {
        debug!(exec_id = %exec.id, dep_count = exec.deps.len(), "loop_deps_satisfied: called");
        for dep_id in &exec.deps {
            if let Some(dep_exec) = self.state.get_execution(dep_id).await? {
                if dep_exec.status != LoopExecutionStatus::Complete {
                    debug!(exec_id = %exec.id, %dep_id, status = ?dep_exec.status, "loop_deps_satisfied: dep not complete");
                    return Ok(false);
                }
                debug!(exec_id = %exec.id, %dep_id, "loop_deps_satisfied: dep complete");
            } else {
                debug!(exec_id = %exec.id, %dep_id, "loop_deps_satisfied: dep not found");
                // Dependency not found - can't proceed
                return Ok(false);
            }
        }
        debug!(exec_id = %exec.id, "loop_deps_satisfied: all deps satisfied");
        Ok(true)
    }

    /// Build context text for title generation from a loop execution
    fn build_title_context(&self, exec: &LoopExecution) -> String {
        let mut parts = Vec::new();

        parts.push(format!("Type: {}", exec.loop_type));

        if let Some(parent_title) = exec.context.get("parent-title").and_then(|v| v.as_str()) {
            parts.push(format!("Parent: {}", parent_title));
        }
        if let Some(phase_name) = exec.context.get("phase-name").and_then(|v| v.as_str()) {
            parts.push(format!("Phase: {}", phase_name));
        }
        if let Some(phase_desc) = exec.context.get("phase-description").and_then(|v| v.as_str()) {
            parts.push(format!("Description: {}", phase_desc));
        }

        parts.join("\n")
    }

    /// Get the output paths for a loop execution based on its type
    ///
    /// Returns (output_file, output_dir) relative to repo root:
    /// - plan: single file at `.taskdaemon/artifacts/plans/{exec_id}/plan.md`
    /// - spec: directory at `.taskdaemon/artifacts/specs/{exec_id}/` (multiple spec files)
    /// - phase: directory at `.taskdaemon/artifacts/phases/{exec_id}/` (multiple phase files)
    /// - ralph: None (produces code, not markdown)
    fn get_output_paths(&self, exec: &LoopExecution) -> (Option<String>, Option<String>) {
        let (file, dir) = match exec.loop_type.as_str() {
            "plan" => {
                let dir = format!(".taskdaemon/artifacts/plans/{}", exec.id);
                let file = format!("{}/plan.md", dir);
                (Some(file), Some(dir))
            }
            "spec" => {
                // Spec loops produce multiple spec files in a directory
                let dir = format!(".taskdaemon/artifacts/specs/{}", exec.id);
                (None, Some(dir))
            }
            "phase" => {
                // Phase loops produce multiple phase files in a directory
                let dir = format!(".taskdaemon/artifacts/phases/{}", exec.id);
                (None, Some(dir))
            }
            _ => return (None, None), // ralph and other types don't produce markdown artifacts
        };
        debug!(exec_id = %exec.id, loop_type = %exec.loop_type, ?file, ?dir, "get_output_paths");
        (file, dir)
    }

    /// Spawn a loop execution as a tokio task
    pub async fn spawn_loop(&mut self, exec: &LoopExecution) -> Result<()> {
        debug!(exec_id = %exec.id, loop_type = %exec.loop_type, "spawn_loop: called");
        // Check if already running
        if self.tasks.contains_key(&exec.id) {
            debug!(exec_id = %exec.id, "spawn_loop: loop already running");
            return Ok(());
        }

        // Generate a unique title for this loop if it doesn't have one
        let mut exec = exec.clone();
        let needs_title = exec.title.as_ref().is_none_or(|t| t.is_empty() || t == &exec.loop_type);
        if needs_title {
            let context = self.build_title_context(&exec);
            if let Some(title) = crate::llm::name_markdown(&self.llm, &context).await {
                info!(exec_id = %exec.id, %title, "Generated title");
                exec.title = Some(title);
            }
        }

        // Set output paths based on loop type (used by template and cascade)
        let (output_file, output_dir) = self.get_output_paths(&exec);
        if let Some(ref path) = output_file {
            exec = exec.with_context_value("output-file", path);
            // Also set artifact tracking fields
            exec.set_artifact(path);
            debug!(exec_id = %exec.id, %path, "spawn_loop: set output-file and artifact path");
        }
        if let Some(ref dir) = output_dir {
            exec = exec.with_context_value("output-dir", dir);
            // If no output-file, use dir as artifact path
            if output_file.is_none() {
                exec.set_artifact(dir);
            }
            debug!(exec_id = %exec.id, %dir, "spawn_loop: set output-dir");
        }
        self.state.update_execution(exec.clone()).await?;

        // Wait for scheduler slot (handles rate limiting and priority queuing)
        // TODO: Extract priority from parent Spec/Plan once we wire that up
        debug!(exec_id = %exec.id, "spawn_loop: waiting for scheduler slot");
        self.scheduler
            .wait_for_slot(&exec.id, crate::domain::Priority::Normal)
            .await
            .context("Failed to acquire scheduler slot")?;
        debug!(exec_id = %exec.id, "spawn_loop: got scheduler slot");

        // Create/verify worktree
        debug!(exec_id = %exec.id, "spawn_loop: creating worktree");
        let worktree_info = self
            .worktree_manager
            .create(&exec.id)
            .await
            .context("Failed to create worktree")?;
        debug!(exec_id = %exec.id, worktree = ?worktree_info.path, "spawn_loop: worktree created");

        // Get loop config for this type
        let loop_config = self.loop_configs.get(&exec.loop_type).cloned().unwrap_or_default();
        debug!(exec_id = %exec.id, has_config = self.loop_configs.contains_key(&exec.loop_type), "spawn_loop: got loop config");

        // Register with coordinator and get a handle
        debug!(exec_id = %exec.id, "spawn_loop: registering with coordinator");
        let coord_handle = self
            .create_coord_handle(&exec.id)
            .await
            .context("Failed to register with coordinator")?;

        // Update execution status to running
        debug!(exec_id = %exec.id, "spawn_loop: updating status to running");
        let mut exec_running = exec.clone();
        exec_running.set_status(LoopExecutionStatus::Running);
        exec_running.set_worktree(worktree_info.path.display().to_string());
        self.state.update_execution(exec_running).await?;

        // Log loop start time clearly for cascade timing analysis
        info!(
            "▶ {} STARTED: {} (worktree: {})",
            exec.loop_type.to_uppercase(),
            &exec.id,
            worktree_info.path.display()
        );

        // Build and spawn engine
        debug!(exec_id = %exec.id, "spawn_loop: building and spawning engine");
        let exec_id = exec.id.clone();
        let loop_type = exec.loop_type.clone();
        let exec_context = exec.context.clone();
        let llm = self.llm.clone();
        let state = self.state.clone();
        let worktree_path = worktree_info.path.clone();
        let repo_root = self.config.repo_root.clone();
        let scheduler = self.scheduler.clone();
        let type_loader = self.type_loader.clone();

        let handle = tokio::spawn(async move {
            debug!(exec_id = %exec_id, "spawn_loop task: starting");
            // Build engine with coordinator, scheduler, execution context, repo root, and state
            let engine =
                LoopEngine::with_coordinator(exec_id.clone(), loop_config, llm, worktree_path.clone(), coord_handle)
                    .with_scheduler(scheduler.clone())
                    .with_execution_context(exec_context)
                    .with_repo_root(repo_root.clone())
                    .with_state(state.clone());

            let result = run_loop_task(engine, state, worktree_path, repo_root, type_loader, loop_type).await;

            // Mark scheduler slot as complete (releases slot for next queued request)
            debug!(exec_id = %exec_id, "spawn_loop task: completing scheduler slot");
            scheduler.complete(&exec_id).await;

            debug!(exec_id = %exec_id, "spawn_loop task: complete");
            result
        });

        self.tasks.insert(exec.id.clone(), handle);
        info!(exec_id = %exec.id, "Spawned loop");
        debug!(exec_id = %exec.id, running_count = self.tasks.len(), "spawn_loop: complete");

        Ok(())
    }

    /// Reap completed tasks and update state
    async fn reap_completed_tasks(&mut self) {
        debug!(task_count = self.tasks.len(), "reap_completed_tasks: called");
        let mut completed_ids = Vec::new();

        for (exec_id, handle) in &self.tasks {
            if handle.is_finished() {
                debug!(exec_id = %exec_id, "reap_completed_tasks: task finished");
                completed_ids.push(exec_id.clone());
            }
        }
        debug!(
            completed_count = completed_ids.len(),
            "reap_completed_tasks: found completed tasks"
        );

        for exec_id in completed_ids {
            if let Some(handle) = self.tasks.remove(&exec_id) {
                debug!(exec_id = %exec_id, "reap_completed_tasks: awaiting task result");
                match handle.await {
                    Ok(LoopTaskResult::Complete { exec_id, iterations }) => {
                        debug!(exec_id = %exec_id, iterations, "reap_completed_tasks: loop completed successfully");
                        info!(exec_id = %exec_id, iterations, "Loop completed successfully");
                    }
                    Ok(LoopTaskResult::Failed { exec_id, reason }) => {
                        debug!(exec_id = %exec_id, %reason, "reap_completed_tasks: loop failed");
                        error!(exec_id = %exec_id, reason = %reason, "Loop failed");
                    }
                    Ok(LoopTaskResult::Stopped { exec_id }) => {
                        debug!(exec_id = %exec_id, "reap_completed_tasks: loop stopped");
                        info!(exec_id = %exec_id, "Loop stopped");
                    }
                    Err(e) => {
                        debug!(exec_id = %exec_id, error = %e, "reap_completed_tasks: loop task panicked");
                        error!(exec_id = %exec_id, error = %e, "Loop task panicked");
                    }
                }

                // Cleanup worktree
                debug!(exec_id = %exec_id, "reap_completed_tasks: removing worktree");
                if let Err(e) = self.worktree_manager.remove(&exec_id).await {
                    warn!(exec_id = %exec_id, error = %e, "Failed to remove worktree");
                }
            }
        }
        debug!("reap_completed_tasks: complete");
    }

    /// Recover interrupted loops on startup
    async fn recover_interrupted_loops(&mut self) -> Result<()> {
        debug!("recover_interrupted_loops: called");
        info!("Scanning for interrupted loops to recover");

        let recoverable_statuses = ["running", "rebasing", "paused"];

        for status in recoverable_statuses {
            debug!(%status, "recover_interrupted_loops: checking status");
            let executions = self.state.list_executions(Some(status.to_string()), None).await?;
            debug!(%status, count = executions.len(), "recover_interrupted_loops: found executions");

            for mut exec in executions {
                info!(exec_id = %exec.id, status = %status, "Found interrupted loop");

                // Check if worktree exists
                if self.worktree_manager.exists(&exec.id) {
                    debug!(exec_id = %exec.id, "recover_interrupted_loops: worktree exists, resuming");
                    // Resume from current state
                    exec.set_status(LoopExecutionStatus::Pending);
                    self.state.update_execution(exec).await?;
                } else {
                    debug!(exec_id = %exec.id, "recover_interrupted_loops: worktree missing, marking failed");
                    // Mark as failed - can't recover without worktree
                    exec.set_status(LoopExecutionStatus::Failed);
                    exec.set_error("Worktree missing during recovery");
                    self.state.update_execution(exec).await?;
                }
            }
        }

        debug!("recover_interrupted_loops: complete");
        Ok(())
    }

    /// Gracefully shutdown all running loops
    async fn shutdown(&mut self) -> Result<()> {
        debug!(task_count = self.tasks.len(), "shutdown: called");
        info!("Shutting down LoopManager with {} active loops", self.tasks.len());

        // Request stop for all running loops via coordinator
        debug!("shutdown: sending stop requests to all loops");
        for exec_id in self.tasks.keys() {
            debug!(exec_id = %exec_id, "shutdown: sending stop request");
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
        debug!(?timeout, "shutdown: waiting for tasks to complete");

        while !self.tasks.is_empty() && tokio::time::Instant::now() < deadline {
            debug!(remaining = self.tasks.len(), "shutdown: waiting for tasks");
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.reap_completed_tasks().await;
        }

        // Force abort remaining tasks
        if !self.tasks.is_empty() {
            debug!(remaining = self.tasks.len(), "shutdown: force aborting remaining tasks");
            warn!("Aborting {} remaining loops after timeout", self.tasks.len());
            for (exec_id, handle) in self.tasks.drain() {
                debug!(exec_id = %exec_id, "shutdown: aborting task");
                handle.abort();

                // Mark as stopped in state
                if let Ok(Some(mut exec)) = self.state.get_execution(&exec_id).await {
                    exec.set_status(LoopExecutionStatus::Stopped);
                    let _ = self.state.update_execution(exec).await;
                }
            }
        } else {
            debug!("shutdown: all tasks completed gracefully");
        }

        // Shutdown coordinator by sending shutdown message
        debug!("shutdown: sending coordinator shutdown");
        let _ = self.coordinator_tx.send(CoordRequest::Shutdown).await;

        info!("LoopManager shutdown complete");
        debug!("shutdown: complete");
        Ok(())
    }

    /// Get the number of running loops
    pub fn running_count(&self) -> usize {
        debug!(count = self.tasks.len(), "running_count: called");
        self.tasks.len()
    }

    /// Get IDs of all running loops
    pub fn running_ids(&self) -> Vec<String> {
        debug!(count = self.tasks.len(), "running_ids: called");
        self.tasks.keys().cloned().collect()
    }

    /// Stop a specific loop
    pub async fn stop_loop(&self, exec_id: &str) -> Result<()> {
        debug!(%exec_id, "stop_loop: called");
        self.coordinator_tx
            .send(CoordRequest::Stop {
                from_exec_id: "loop_manager".to_string(),
                target_exec_id: exec_id.to_string(),
                reason: "User requested stop".to_string(),
            })
            .await
            .map_err(|_| eyre::eyre!("Coordinator channel closed"))?;

        debug!(%exec_id, "stop_loop: stop request sent");
        Ok(())
    }
}

/// Run a loop task and handle completion
///
/// On successful completion, merges the worktree branch to main and triggers cascade.
async fn run_loop_task(
    mut engine: LoopEngine,
    state: StateManager,
    worktree_path: PathBuf,
    repo_root: PathBuf,
    type_loader: Arc<RwLock<LoopLoader>>,
    loop_type: String,
) -> LoopTaskResult {
    let exec_id = engine.exec_id.clone();
    debug!(exec_id = %exec_id, %loop_type, "run_loop_task: called");

    match engine.run().await {
        Ok(crate::r#loop::IterationResult::Complete { iterations }) => {
            debug!(exec_id = %exec_id, iterations, "run_loop_task: loop completed successfully");
            // Get execution details for merge commit message and cascade
            let exec_data = state.get_execution(&exec_id).await.ok().flatten();
            let spec_title = exec_data
                .as_ref()
                .and_then(|e| e.context.get("title").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_else(|| "Completed work".to_string());

            // Only merge for code-producing loops (phase, ralph)
            // Plan and Spec loops produce markdown docs, not code to merge
            let should_merge = matches!(loop_type.as_str(), "phase" | "ralph");

            if !should_merge {
                debug!(exec_id = %exec_id, loop_type = %loop_type, "run_loop_task: skipping merge for doc loop");
                // Skip merge - just mark complete and trigger cascade
                if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                    exec.set_status(LoopExecutionStatus::Complete);
                    exec.set_artifact_status("complete");
                    exec.iteration = engine.current_iteration();
                    exec.progress = engine.get_progress();
                    let _ = state.update_execution(exec.clone()).await;

                    // Trigger cascade: create Loop record and spawn child executions
                    debug!(exec_id = %exec_id, "run_loop_task: triggering cascade (no merge)");
                    trigger_cascade(&state, &type_loader, &exec, &loop_type).await;
                }
                return LoopTaskResult::Complete { exec_id, iterations };
            }

            // Merge to main before marking complete (for code loops)
            debug!(exec_id = %exec_id, "run_loop_task: merging to main");
            match merge_to_main(&repo_root, &worktree_path, &exec_id, &spec_title).await {
                Ok(MergeResult::Success) => {
                    debug!(exec_id = %exec_id, "run_loop_task: merge successful");
                    info!(exec_id = %exec_id, "Successfully merged to main");
                    // Update state to complete with progress
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Complete);
                        exec.set_artifact_status("complete");
                        exec.iteration = engine.current_iteration();
                        exec.progress = engine.get_progress();
                        let _ = state.update_execution(exec.clone()).await;

                        // Trigger cascade: create Loop record and spawn child executions
                        debug!(exec_id = %exec_id, "run_loop_task: triggering cascade");
                        trigger_cascade(&state, &type_loader, &exec, &loop_type).await;
                    }
                    LoopTaskResult::Complete { exec_id, iterations }
                }
                Ok(MergeResult::Conflict { message }) => {
                    debug!(exec_id = %exec_id, %message, "run_loop_task: merge conflict");
                    warn!(exec_id = %exec_id, "Merge conflict: {}", message);
                    // Mark as blocked - needs manual intervention
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Blocked);
                        exec.set_artifact_status("failed");
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
                    debug!(exec_id = %exec_id, %message, "run_loop_task: push failed");
                    warn!(exec_id = %exec_id, "Push failed: {}", message);
                    // Mark as failed - push issue
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Failed);
                        exec.set_artifact_status("failed");
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
                    debug!(exec_id = %exec_id, error = %e, "run_loop_task: merge error");
                    error!(exec_id = %exec_id, "Merge error: {}", e);
                    if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                        exec.set_status(LoopExecutionStatus::Failed);
                        exec.set_artifact_status("failed");
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
            debug!(exec_id = %exec_id, "run_loop_task: loop interrupted");
            // Update state to stopped with progress (artifact status stays draft)
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Stopped);
                exec.iteration = engine.current_iteration();
                exec.progress = engine.get_progress();
                let _ = state.update_execution(exec).await;
            }
            LoopTaskResult::Stopped { exec_id }
        }
        Ok(crate::r#loop::IterationResult::Error { message, .. }) => {
            debug!(exec_id = %exec_id, %message, "run_loop_task: loop error");
            // Update state to failed with progress
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_artifact_status("failed");
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
            debug!(exec_id = %exec_id, "run_loop_task: unexpected loop result");
            // Other results treated as failure
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_artifact_status("failed");
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
            debug!(exec_id = %exec_id, error = %e, "run_loop_task: loop run error");
            // Update state to failed with progress
            if let Ok(Some(mut exec)) = state.get_execution(&exec_id).await {
                exec.set_status(LoopExecutionStatus::Failed);
                exec.set_artifact_status("failed");
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

/// Trigger cascade after execution completion
///
/// Creates a Loop record with status Ready and uses CascadeHandler to spawn child executions.
/// The child executions will be created in Pending state and picked up by poll_and_spawn.
async fn trigger_cascade(
    state: &StateManager,
    type_loader: &Arc<RwLock<LoopLoader>>,
    exec: &LoopExecution,
    loop_type: &str,
) {
    debug!(exec_id = %exec.id, %loop_type, "trigger_cascade: called");
    // Get the execution title for the Loop record
    let title = exec.context.get("title").and_then(|v| v.as_str()).unwrap_or(&exec.id);
    debug!(exec_id = %exec.id, %title, "trigger_cascade: got title");

    // Get the output file path (plan.md, spec file, etc.)
    let output_file = exec.context.get("output-file").and_then(|v| v.as_str());

    // Create a Loop record representing the completed work
    let mut loop_record = Loop::new(loop_type, title);
    loop_record.set_status(LoopStatus::Ready);

    // Link to output file if present
    if let Some(file) = output_file {
        debug!(exec_id = %exec.id, %file, "trigger_cascade: linking output file");
        loop_record = loop_record.with_file(file);
    }

    // Store the execution ID for reference in context
    if let Some(obj) = loop_record.context.as_object_mut() {
        obj.insert("exec_id".to_string(), serde_json::json!(exec.id));
    }

    // Create the Loop record in state
    debug!(exec_id = %exec.id, loop_id = %loop_record.id, "trigger_cascade: creating loop record");
    if let Err(e) = state.create_loop(loop_record.clone()).await {
        debug!(exec_id = %exec.id, error = %e, "trigger_cascade: failed to create loop record");
        warn!(exec_id = %exec.id, loop_type = %loop_type, error = %e, "Failed to create Loop record for cascade");
        return;
    }

    info!(
        exec_id = %exec.id,
        loop_id = %loop_record.id,
        loop_type = %loop_type,
        "Created Loop record for cascade"
    );

    // Create cascade handler and trigger child execution creation
    debug!(exec_id = %exec.id, "trigger_cascade: calling on_loop_ready");
    let cascade = CascadeHandler::new(Arc::new(state.clone()), type_loader.clone());
    match cascade.on_loop_ready(&loop_record, &exec.id).await {
        Ok(children) => {
            if children.is_empty() {
                debug!(loop_id = %loop_record.id, "trigger_cascade: no child types");
                info!(loop_id = %loop_record.id, loop_type = %loop_type, "No child loop types defined, cascade complete");
            } else {
                debug!(loop_id = %loop_record.id, child_count = children.len(), "trigger_cascade: created children");
                info!(
                    loop_id = %loop_record.id,
                    loop_type = %loop_type,
                    child_count = children.len(),
                    "Cascade created child executions"
                );
            }
        }
        Err(e) => {
            debug!(loop_id = %loop_record.id, error = %e, "trigger_cascade: cascade failed");
            error!(loop_id = %loop_record.id, loop_type = %loop_type, error = %e, "Cascade failed to create child executions");
        }
    }
    debug!(exec_id = %exec.id, "trigger_cascade: complete");
}

/// Validate a dependency graph for cycles
///
/// Uses DFS to detect cycles. Returns Ok(()) if no cycles, Err with cycle info if found.
pub fn validate_dependency_graph<'a>(loops: impl IntoIterator<Item = &'a Loop>) -> Result<(), Vec<String>> {
    debug!("validate_dependency_graph: called");
    let loop_map: HashMap<&str, &Loop> = loops.into_iter().map(|l| (l.id.as_str(), l)).collect();
    debug!(loop_count = loop_map.len(), "validate_dependency_graph: built loop map");

    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut cycle_path = Vec::new();

    for loop_id in loop_map.keys() {
        if !visited.contains(loop_id)
            && has_cycle_dfs(loop_id, &loop_map, &mut visited, &mut rec_stack, &mut cycle_path)
        {
            debug!(?cycle_path, "validate_dependency_graph: cycle detected");
            return Err(cycle_path);
        }
    }

    debug!("validate_dependency_graph: no cycles found");
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
    debug!(%node, "has_cycle_dfs: called");
    visited.insert(node);
    rec_stack.insert(node);
    cycle_path.push(node.to_string());

    if let Some(record) = graph.get(node) {
        for dep_id in &record.deps {
            if !visited.contains(dep_id.as_str()) {
                debug!(%node, %dep_id, "has_cycle_dfs: visiting unvisited dep");
                if graph.contains_key(dep_id.as_str())
                    && has_cycle_dfs(dep_id.as_str(), graph, visited, rec_stack, cycle_path)
                {
                    debug!(%node, %dep_id, "has_cycle_dfs: cycle found in subtree");
                    return true;
                }
            } else if rec_stack.contains(dep_id.as_str()) {
                debug!(%node, %dep_id, "has_cycle_dfs: back edge found - cycle detected");
                cycle_path.push(dep_id.clone());
                return true;
            } else {
                debug!(%node, %dep_id, "has_cycle_dfs: dep already visited, no cycle");
            }
        }
    } else {
        debug!(%node, "has_cycle_dfs: node not in graph");
    }

    rec_stack.remove(node);
    cycle_path.pop();
    debug!(%node, "has_cycle_dfs: no cycle from this node");
    false
}

/// Topologically sort loops by dependencies
///
/// Returns loops in execution order (dependencies first).
/// The returned Vec contains indices into the input slice.
pub fn topological_sort(loops: &[Loop]) -> Result<Vec<usize>, Vec<String>> {
    debug!(loop_count = loops.len(), "topological_sort: called");
    // First validate no cycles
    validate_dependency_graph(loops)?;
    debug!("topological_sort: no cycles");

    // Build index map: id -> index in loops
    let index_map: HashMap<&str, usize> = loops.iter().enumerate().map(|(i, l)| (l.id.as_str(), i)).collect();

    let mut visited = HashSet::new();
    let mut result = Vec::new();

    for idx in 0..loops.len() {
        topo_dfs_idx(idx, loops, &index_map, &mut visited, &mut result);
    }

    debug!(result_len = result.len(), "topological_sort: complete");
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
    debug!(idx, "topo_dfs_idx: called");
    if visited.contains(&idx) {
        debug!(idx, "topo_dfs_idx: already visited");
        return;
    }

    visited.insert(idx);

    let record = &loops[idx];
    for dep_id in &record.deps {
        if let Some(&dep_idx) = index_map.get(dep_id.as_str()) {
            debug!(idx, dep_idx, %dep_id, "topo_dfs_idx: visiting dependency");
            topo_dfs_idx(dep_idx, loops, index_map, visited, result);
        } else {
            debug!(idx, %dep_id, "topo_dfs_idx: dependency not in index map");
        }
    }
    debug!(idx, "topo_dfs_idx: adding to result");
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
        assert_eq!(config.poll_interval_secs, 60); // Increased for event-driven pickup
        assert_eq!(config.shutdown_timeout_secs, 60);
    }
}
