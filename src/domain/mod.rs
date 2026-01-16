//! Domain types for TaskDaemon
//!
//! Core domain types: Plan, Spec, LoopExecution
//! All implement the Record trait for TaskStore persistence.

mod execution;
mod id;
mod plan;
mod priority;
mod spec;

pub use execution::{LoopExecution, LoopExecutionStatus};
pub use id::{DomainId, IdResolver};
pub use plan::{Plan, PlanStatus};
pub use priority::Priority;
pub use spec::{Phase, PhaseStatus, Spec, SpecStatus};

// Re-export taskstore types for convenience
pub use taskstore::{Filter, FilterOp, IndexValue, Record, Store};
