//! Scheduler implementation

use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use eyre::eyre;
use tokio::sync::{Mutex, Notify};
use tracing::{debug, warn};

use crate::domain::Priority;

use super::config::SchedulerConfig;
use super::queue::{QueueEntry, QueueEntryStatus, QueueState, ScheduleResult, ScheduledRequest, SchedulerStats};

/// Internal state protected by mutex
struct SchedulerInner {
    /// Priority queue of waiting requests
    queue: BinaryHeap<ScheduledRequest>,

    /// Currently running requests
    running: HashMap<String, ScheduledRequest>,

    /// Request timestamps for rate limiting (sliding window)
    request_times: VecDeque<Instant>,

    /// Statistics
    stats: SchedulerStats,
}

/// The Scheduler manages loop execution with priority queuing,
/// concurrency limits, and rate limiting.
pub struct Scheduler {
    config: SchedulerConfig,
    inner: Mutex<SchedulerInner>,
    notify: Notify,
}

impl Scheduler {
    /// Create a new scheduler with the given configuration
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(SchedulerInner {
                queue: BinaryHeap::new(),
                running: HashMap::new(),
                request_times: VecDeque::new(),
                stats: SchedulerStats::default(),
            }),
            notify: Notify::new(),
        }
    }

    /// Attempt to schedule a request
    pub async fn schedule(&self, exec_id: &str, priority: Priority) -> ScheduleResult {
        let mut inner = self.inner.lock().await;

        // Check if already running
        if inner.running.contains_key(exec_id) {
            return ScheduleResult::Rejected {
                reason: "Already running".to_string(),
            };
        }

        // Check if already queued
        if inner.queue.iter().any(|r| r.exec_id == exec_id) {
            return ScheduleResult::Rejected {
                reason: "Already queued".to_string(),
            };
        }

        let now = Instant::now();

        // Prune old request times (outside rate window)
        let window_start = now - self.config.rate_window();
        while inner.request_times.front().map(|t| *t < window_start).unwrap_or(false) {
            inner.request_times.pop_front();
        }

        // Check rate limit
        if inner.request_times.len() >= self.config.max_requests_per_window as usize {
            let oldest = inner.request_times.front().unwrap();
            let retry_after = self.config.rate_window() - (now - *oldest);
            inner.stats.total_rate_limited += 1;
            debug!(exec_id, ?retry_after, "Rate limited");
            return ScheduleResult::RateLimited { retry_after };
        }

        // Check concurrent limit
        if inner.running.len() < self.config.max_concurrent {
            // Can run immediately
            let request = ScheduledRequest {
                exec_id: exec_id.to_string(),
                priority,
                submitted_at: now,
                started_at: Some(now),
            };

            inner.running.insert(exec_id.to_string(), request);
            inner.request_times.push_back(now);
            inner.stats.total_scheduled += 1;
            inner.stats.peak_concurrent = inner.stats.peak_concurrent.max(inner.running.len());

            debug!(exec_id, ?priority, "Scheduled immediately");
            return ScheduleResult::Ready;
        }

        // Queue the request
        let request = ScheduledRequest {
            exec_id: exec_id.to_string(),
            priority,
            submitted_at: now,
            started_at: None,
        };

        inner.queue.push(request);
        inner.stats.peak_queue_depth = inner.stats.peak_queue_depth.max(inner.queue.len());

        // Calculate position (approximate since heap doesn't have index)
        let position = inner
            .queue
            .iter()
            .filter(|r| r.priority > priority || (r.priority == priority && r.submitted_at < now))
            .count()
            + 1;

        // Estimate wait time based on average completion time
        let avg_completion_ms = if inner.stats.total_completed > 0 {
            inner.stats.total_wait_time_ms / inner.stats.total_completed
        } else {
            30_000 // Default 30s estimate
        };

        let estimated_wait =
            Duration::from_millis((position as u64 * avg_completion_ms) / self.config.max_concurrent as u64);

        debug!(exec_id, position, ?estimated_wait, "Queued");
        ScheduleResult::Queued {
            position,
            estimated_wait,
        }
    }

    /// Wait until a slot is available for this request
    pub async fn wait_for_slot(&self, exec_id: &str, priority: Priority) -> eyre::Result<()> {
        loop {
            match self.schedule(exec_id, priority).await {
                ScheduleResult::Ready => return Ok(()),
                ScheduleResult::Queued { .. } => {
                    // Wait for notification that a slot opened
                    self.notify.notified().await;
                }
                ScheduleResult::RateLimited { retry_after } => {
                    tokio::time::sleep(retry_after).await;
                }
                ScheduleResult::Rejected { reason } => {
                    return Err(eyre!("Schedule rejected: {}", reason));
                }
            }
        }
    }

    /// Mark a request as complete, opening a slot
    pub async fn complete(&self, exec_id: &str) {
        let mut inner = self.inner.lock().await;

        if let Some(request) = inner.running.remove(exec_id) {
            if let Some(started) = request.started_at {
                let wait_time = started.elapsed().as_millis() as u64;
                inner.stats.total_wait_time_ms += wait_time;
            }
            inner.stats.total_completed += 1;
            debug!(exec_id, "Completed");
        }

        // Try to start next queued request
        if let Some(mut next) = inner.queue.pop() {
            debug!(exec_id = %next.exec_id, ?next.priority, "Promoting from queue");
            next.started_at = Some(Instant::now());
            inner.running.insert(next.exec_id.clone(), next);
            inner.request_times.push_back(Instant::now());
        }

        drop(inner);

        // Notify waiters that a slot may be available
        self.notify.notify_waiters();
    }

    /// Handle external rate limit (429 from API)
    pub async fn handle_rate_limit(&self, retry_after: Duration) {
        warn!(?retry_after, "Received rate limit from API");

        let mut inner = self.inner.lock().await;

        // Fill the rate window to block new requests
        let now = Instant::now();
        while inner.request_times.len() < self.config.max_requests_per_window as usize {
            inner.request_times.push_back(now);
        }

        inner.stats.total_rate_limited += 1;

        drop(inner);

        // Sleep for retry period
        tokio::time::sleep(retry_after).await;
    }

    /// Get current queue state for TUI
    pub async fn queue_state(&self) -> QueueState {
        let inner = self.inner.lock().await;

        QueueState {
            running: inner.running.len(),
            queued: inner.queue.len(),
            rate_limited: inner.request_times.len() >= self.config.max_requests_per_window as usize,
            stats: inner.stats.clone(),
        }
    }

    /// Get detailed queue for TUI display
    pub async fn queue_details(&self) -> Vec<QueueEntry> {
        let inner = self.inner.lock().await;
        let now = Instant::now();

        let mut entries: Vec<_> = inner
            .running
            .values()
            .map(|r| QueueEntry {
                exec_id: r.exec_id.clone(),
                priority: r.priority,
                status: QueueEntryStatus::Running,
                wait_time: r.started_at.map(|s| now - s),
            })
            .chain(inner.queue.iter().map(|r| QueueEntry {
                exec_id: r.exec_id.clone(),
                priority: r.priority,
                status: QueueEntryStatus::Queued,
                wait_time: Some(now - r.submitted_at),
            }))
            .collect();

        entries.sort_by(|a, b| b.priority.cmp(&a.priority));
        entries
    }

    /// Get the scheduler statistics
    pub async fn stats(&self) -> SchedulerStats {
        let inner = self.inner.lock().await;
        inner.stats.clone()
    }

    /// Cancel a queued request (remove from queue)
    pub async fn cancel(&self, exec_id: &str) -> bool {
        let mut inner = self.inner.lock().await;

        // Check if running - can't cancel running
        if inner.running.contains_key(exec_id) {
            return false;
        }

        // Remove from queue
        let original_len = inner.queue.len();
        let queue_vec: Vec<_> = inner.queue.drain().filter(|r| r.exec_id != exec_id).collect();
        inner.queue = queue_vec.into_iter().collect();

        original_len != inner.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_concurrent_limit() {
        let scheduler = Scheduler::new(SchedulerConfig {
            max_concurrent: 2,
            ..Default::default()
        });

        // First two should be ready
        assert!(matches!(
            scheduler.schedule("a", Priority::Normal).await,
            ScheduleResult::Ready
        ));
        assert!(matches!(
            scheduler.schedule("b", Priority::Normal).await,
            ScheduleResult::Ready
        ));

        // Third should be queued
        assert!(matches!(
            scheduler.schedule("c", Priority::Normal).await,
            ScheduleResult::Queued { position: 1, .. }
        ));

        // Complete one, third should get slot (auto-promoted)
        scheduler.complete("a").await;

        // Verify state
        let state = scheduler.queue_state().await;
        assert_eq!(state.running, 2); // b and c
        assert_eq!(state.queued, 0);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let scheduler = Scheduler::new(SchedulerConfig {
            max_concurrent: 1,
            ..Default::default()
        });

        // Fill the slot
        scheduler.schedule("running", Priority::Normal).await;

        // Queue low, normal, high
        scheduler.schedule("low", Priority::Low).await;
        scheduler.schedule("normal", Priority::Normal).await;
        scheduler.schedule("high", Priority::High).await;

        // Complete running, high should be next
        scheduler.complete("running").await;

        let details = scheduler.queue_details().await;
        let running: Vec<_> = details
            .iter()
            .filter(|e| e.status == QueueEntryStatus::Running)
            .collect();

        assert_eq!(running[0].exec_id, "high");
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let scheduler = Scheduler::new(SchedulerConfig {
            max_concurrent: 10,
            max_requests_per_window: 3,
            rate_window_secs: 60,
            ..Default::default()
        });

        // First three should work
        scheduler.schedule("a", Priority::Normal).await;
        scheduler.schedule("b", Priority::Normal).await;
        scheduler.schedule("c", Priority::Normal).await;

        // Fourth should be rate limited
        assert!(matches!(
            scheduler.schedule("d", Priority::Normal).await,
            ScheduleResult::RateLimited { .. }
        ));
    }

    #[tokio::test]
    async fn test_duplicate_rejection() {
        let scheduler = Scheduler::new(SchedulerConfig::default());

        // First should succeed
        assert!(matches!(
            scheduler.schedule("test", Priority::Normal).await,
            ScheduleResult::Ready
        ));

        // Second with same ID should be rejected (already running)
        assert!(matches!(
            scheduler.schedule("test", Priority::Normal).await,
            ScheduleResult::Rejected { .. }
        ));
    }

    #[tokio::test]
    async fn test_cancel() {
        let scheduler = Scheduler::new(SchedulerConfig {
            max_concurrent: 1,
            ..Default::default()
        });

        // Fill the slot and queue another
        scheduler.schedule("running", Priority::Normal).await;
        scheduler.schedule("queued", Priority::Normal).await;

        // Cancel the queued one
        assert!(scheduler.cancel("queued").await);

        // Try to cancel running (should fail)
        assert!(!scheduler.cancel("running").await);

        // Complete running, queue should be empty
        scheduler.complete("running").await;

        let state = scheduler.queue_state().await;
        assert_eq!(state.running, 0);
        assert_eq!(state.queued, 0);
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let scheduler = Scheduler::new(SchedulerConfig {
            max_concurrent: 2,
            ..Default::default()
        });

        scheduler.schedule("a", Priority::Normal).await;
        scheduler.schedule("b", Priority::Normal).await;

        scheduler.complete("a").await;
        scheduler.complete("b").await;

        let stats = scheduler.stats().await;
        assert_eq!(stats.total_scheduled, 2);
        assert_eq!(stats.total_completed, 2);
        assert_eq!(stats.peak_concurrent, 2);
    }
}
