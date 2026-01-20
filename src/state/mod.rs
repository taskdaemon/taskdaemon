//! State management with actor pattern
//!
//! StateManager owns the TaskStore and processes messages via channels,
//! providing thread-safe access to persistent state.

mod manager;
mod messages;
mod recovery;

pub use manager::{DaemonMetrics, StateEvent, StateManager, read_state_version};
pub use messages::{StateCommand, StateError, StateResponse};
pub use recovery::{RecoveryStats, recover, scan_for_recovery};
