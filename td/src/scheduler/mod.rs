//! Scheduler for loop execution
//!
//! Manages loop execution with priority queuing, concurrency limits,
//! and rate limiting in a single component.

mod config;
mod core;
mod queue;

pub use config::SchedulerConfig;
pub use core::Scheduler;
pub use queue::{QueueEntry, QueueEntryStatus, QueueState, ScheduleResult, ScheduledRequest};
