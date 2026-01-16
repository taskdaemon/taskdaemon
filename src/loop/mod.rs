//! Loop execution module for TaskDaemon
//!
//! The Loop Engine executes Ralph Wiggum iterations: prompt → LLM → tools → repeat
//! until validation passes. Each iteration starts with a fresh LLM context window.
//! State persists in files and git, not memory.

mod config;
mod engine;
mod validation;

pub use config::LoopConfig;
#[allow(unused_imports)]
pub use engine::{IterationResult, LoopEngine, LoopStatus};
#[allow(unused_imports)]
pub use validation::ValidationResult;
