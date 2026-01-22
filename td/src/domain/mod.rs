//! Domain types for TaskDaemon
//!
//! Core domain types: Loop, LoopExecution, IterationLog
//! All implement the Record trait for TaskStore persistence.
//!
//! The generic Loop type works with any loop type defined in YAML configuration.
//! The `type` field determines behavior at runtime.

#[allow(unused_imports)]
use tracing::debug;

mod id;
mod iteration_log;
mod priority;
mod record;
mod run;

pub use id::{DomainId, IdResolver};
pub use iteration_log::{IterationLog, ToolCallSummary};
pub use priority::Priority;
pub use record::{Loop, LoopStatus, Phase, PhaseStatus};
pub use run::{LoopExecution, LoopExecutionStatus, LoopRun, LoopRunStatus};

// Re-export taskstore types for convenience
pub use taskstore::{Filter, FilterOp, IndexValue, Record, Store};
