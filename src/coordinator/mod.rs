//! Coordinator for inter-loop communication
//!
//! The Coordinator mediates all inter-loop communication via three primitives:
//! - **Alert:** Broadcast event to all subscribers
//! - **Query:** Request/reply with timeout
//! - **Share:** Point-to-point data transfer

mod config;
mod core;
mod handle;
mod messages;

pub use config::CoordinatorConfig;
pub use core::Coordinator;
pub use handle::CoordinatorHandle;
pub use messages::{CoordMessage, CoordRequest, CoordinatorMetrics, QueryPayload};
