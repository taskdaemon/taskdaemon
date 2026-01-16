//! Loop execution module for TaskDaemon
//!
//! The Loop Engine executes Ralph Wiggum iterations: prompt → LLM → tools → repeat
//! until validation passes. Each iteration starts with a fresh LLM context window.
//! State persists in files and git, not memory.

mod cascade;
mod config;
mod engine;
mod manager;
mod metrics;
mod type_loader;
mod validation;

pub use cascade::CascadeHandler;
pub use config::LoopConfig;
#[allow(unused_imports)]
pub use engine::{IterationResult, LoopEngine, LoopStatus};
pub use manager::{LoopManager, LoopManagerConfig, LoopTaskResult, topological_sort, validate_dependency_graph};
pub use metrics::{GlobalSummary, IterationTimer, LoopMetrics, LoopStats, TypeMetrics};
pub use type_loader::{LoopLoader, LoopType};
#[allow(unused_imports)]
pub use validation::ValidationResult;
