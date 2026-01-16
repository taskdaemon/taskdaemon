//! Queue types for the scheduler

use std::time::{Duration, Instant};

use crate::domain::Priority;

/// Result of scheduling attempt
#[derive(Debug, Clone)]
pub enum ScheduleResult {
    /// Can execute immediately
    Ready,

    /// Queued, waiting for slot
    Queued { position: usize, estimated_wait: Duration },

    /// Rate limited, try again later
    RateLimited { retry_after: Duration },

    /// Request rejected (invalid, duplicate, etc.)
    Rejected { reason: String },
}

/// A scheduled request
#[derive(Debug, Clone)]
pub struct ScheduledRequest {
    pub exec_id: String,
    pub priority: Priority,
    pub submitted_at: Instant,
    pub started_at: Option<Instant>,
}

impl ScheduledRequest {
    /// Create a new scheduled request
    pub fn new(exec_id: impl Into<String>, priority: Priority) -> Self {
        Self {
            exec_id: exec_id.into(),
            priority,
            submitted_at: Instant::now(),
            started_at: None,
        }
    }
}

impl Eq for ScheduledRequest {}

impl PartialEq for ScheduledRequest {
    fn eq(&self, other: &Self) -> bool {
        self.exec_id == other.exec_id
    }
}

impl Ord for ScheduledRequest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority first, then earlier submission
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.submitted_at.cmp(&self.submitted_at))
    }
}

impl PartialOrd for ScheduledRequest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Statistics for the scheduler
#[derive(Debug, Default, Clone)]
pub struct SchedulerStats {
    pub total_scheduled: u64,
    pub total_completed: u64,
    pub total_rate_limited: u64,
    pub total_wait_time_ms: u64,
    pub peak_queue_depth: usize,
    pub peak_concurrent: usize,
}

/// Queue state for TUI display
#[derive(Debug, Clone)]
pub struct QueueState {
    pub running: usize,
    pub queued: usize,
    pub rate_limited: bool,
    pub stats: SchedulerStats,
}

/// Queue entry for TUI display
#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub exec_id: String,
    pub priority: Priority,
    pub status: QueueEntryStatus,
    pub wait_time: Option<Duration>,
}

/// Status of a queue entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueEntryStatus {
    Running,
    Queued,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduled_request_ordering() {
        let high = ScheduledRequest::new("high", Priority::High);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let normal = ScheduledRequest::new("normal", Priority::Normal);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let low = ScheduledRequest::new("low", Priority::Low);

        // Higher priority should come first
        assert!(high > normal);
        assert!(normal > low);
    }

    #[test]
    fn test_scheduled_request_same_priority_fifo() {
        let first = ScheduledRequest::new("first", Priority::Normal);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let second = ScheduledRequest::new("second", Priority::Normal);

        // Earlier submission should come first (so it's "greater" in the heap)
        assert!(first > second);
    }

    #[test]
    fn test_scheduled_request_equality() {
        let a = ScheduledRequest::new("test", Priority::Normal);
        let b = ScheduledRequest::new("test", Priority::High);

        // Same exec_id means equal
        assert_eq!(a, b);
    }
}
