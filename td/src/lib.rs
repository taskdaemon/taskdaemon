//! TaskDaemon - Extensible Ralph Wiggum Loop Orchestrator
//!
//! TaskDaemon is a framework for orchestrating concurrent autonomous agentic
//! workflows using the Ralph Wiggum loop pattern. Each loop restarts iterations
//! with fresh context windows (preventing context rot) while persisting state
//! in files and git.
//!
//! # Core Concepts
//!
//! - **Fresh Context Always**: Every iteration starts a new API conversation
//! - **State in Files**: Progress persists in git and TaskStore, not memory
//! - **Concrete Validation**: Completion determined by exit codes, not LLM promises
//! - **Massive Parallelism**: Tokio async tasks enable 50+ concurrent loops
//!
//! # Modules
//!
//! - [`llm`] - LLM client trait and Anthropic implementation
//! - [`progress`] - Cross-iteration progress tracking
//! - [`tools`] - Tool system for file/command operations
//! - [`r#loop`] - Loop execution engine
//! - [`config`] - Configuration types and loading
//! - [`cli`] - Command-line interface

// Phase 1 infrastructure - these types are used in later phases when CLI is wired up
#![allow(dead_code)]

pub mod cli;
pub mod config;
pub mod coordinator;
pub mod daemon;
pub mod domain;
pub mod events;
pub mod llm;
pub mod progress;
pub mod prompts;
pub mod scheduler;
pub mod state;
pub mod tools;
pub mod tui;
pub mod validation;
pub mod watcher;
pub mod worktree;

// Note: 'loop' is a reserved keyword, so we use r#loop
#[path = "loop/mod.rs"]
pub mod r#loop;

// Re-export commonly used types
pub use config::{Config, LlmConfig};
pub use coordinator::{
    CoordMessage, CoordRequest, Coordinator, CoordinatorConfig, CoordinatorHandle, CoordinatorMetrics, EventStore,
    PersistedEvent, PersistedEventType,
};
pub use domain::{
    DomainId, Filter, FilterOp, IndexValue, Loop, LoopExecution, LoopExecutionStatus, LoopStatus, Phase, PhaseStatus,
    Priority, Record, Store,
};
pub use llm::{
    AnthropicClient, CompletionRequest, CompletionResponse, LlmClient, LlmError, OpenAIClient, create_client,
};
pub use r#loop::{
    CascadeHandler, GlobalSummary, IterationResult, IterationTimer, LoopConfig, LoopEngine, LoopLoader, LoopManager,
    LoopManagerConfig, LoopMetrics, LoopStats, LoopTaskResult, LoopType, TypeMetrics, topological_sort,
    validate_dependency_graph,
};
pub use progress::{IterationContext, ProgressStrategy, SystemCapturedProgress};
pub use prompts::{FocusArea, PromptContext, PromptLoader};
pub use scheduler::{QueueEntry, QueueEntryStatus, QueueState, ScheduleResult, Scheduler, SchedulerConfig};
pub use state::{RecoveryStats, StateCommand, StateError, StateManager, StateResponse, recover, scan_for_recovery};
pub use tools::{Tool, ToolContext, ToolError, ToolExecutor, ToolResult};
pub use validation::{PassResult, PlanRefinementContext, ReviewPass};
pub use watcher::{MainWatcher, WatcherConfig};
pub use worktree::{MergeResult, WorktreeConfig, WorktreeError, WorktreeInfo, WorktreeManager, merge_to_main};

// Events module re-exports
pub use events::{
    EventBus, EventEmitter, EventLogEntry, EventLogger, IterationOutcome as EventIterationOutcome, TdEvent,
    create_event_bus, read_execution_events, spawn_event_logger,
};
