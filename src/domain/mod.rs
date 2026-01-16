//! Domain types for TaskDaemon
//!
//! Core domain types: Loop, LoopExecution
//! All implement the Record trait for TaskStore persistence.
//!
//! The generic Loop type works with any loop type defined in YAML configuration.
//! The `type` field determines behavior at runtime.

mod execution;
mod id;
mod priority;
mod record;

pub use execution::{LoopExecution, LoopExecutionStatus};
pub use id::{DomainId, IdResolver};
pub use priority::Priority;
pub use record::{Loop, LoopStatus, Phase, PhaseStatus};

// Re-export taskstore types for convenience
pub use taskstore::{Filter, FilterOp, IndexValue, Record, Store};
