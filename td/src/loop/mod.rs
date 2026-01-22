//! Loop execution module for TaskDaemon
//!
//! The Loop Engine executes Ralph Wiggum iterations: prompt → LLM → tools → repeat
//! until validation passes. Each iteration starts with a fresh LLM context window.
//! State persists in files and git, not memory.
//!
//! The ExploreTask provides a lighter-weight read-only exploration capability
//! for investigating codebases without the full Ralph loop pattern.

mod cascade;
mod config;
mod engine;
mod explore;
mod manager;
mod metrics;
mod type_loader;
mod validation;

pub use cascade::CascadeHandler;
pub use config::LoopConfig;
#[allow(unused_imports)]
pub use engine::{IterationResult, LoopEngine, LoopStatus};
pub use explore::{ExploreTask, generate_explore_id};
pub use manager::{
    LoopManager, LoopManagerConfig, LoopTaskResult, TaskManager, TaskManagerConfig, TaskResult,
    topological_sort, validate_dependency_graph,
};
pub use metrics::{GlobalSummary, IterationTimer, LoopMetrics, LoopStats, TypeMetrics};
pub use type_loader::{LoopLoader, LoopType};
#[allow(unused_imports)]
pub use validation::ValidationResult;
