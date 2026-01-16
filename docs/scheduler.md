# Scheduler Specification

**Author:** Scott A. Idler
**Date:** 2026-01-15
**Status:** Implementation Spec

---

## Summary

The Scheduler manages loop execution with priority queuing, concurrency limits, and rate limiting in a single component. Replaces the simpler semaphore approach from earlier designs for better visibility and control.

---

## Why Not Just a Semaphore?

A simple `Semaphore(10)` for API calls works but lacks:

1. **Priority** - Urgent loops wait behind less important ones
2. **Visibility** - No queue state for TUI
3. **Rate limiting** - Can't respond to 429s globally
4. **Statistics** - No metrics on wait times, queue depth

The Scheduler provides all of this while remaining simple.

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ LoopManager                                                       │
│                                                                   │
│  Loop 1 ──┐                                                      │
│  Loop 2 ──┼──> schedule() ──> Scheduler ──> Ready/Queued/Limited │
│  Loop 3 ──┘                      │                               │
│                                  │                               │
│                           ┌──────▼──────┐                        │
│                           │ Priority    │                        │
│                           │ Queue       │                        │
│                           │ ┌─────────┐ │                        │
│                           │ │ High    │ │                        │
│                           │ ├─────────┤ │                        │
│                           │ │ Normal  │ │                        │
│                           │ ├─────────┤ │                        │
│                           │ │ Low     │ │                        │
│                           │ └─────────┘ │                        │
│                           └─────────────┘                        │
│                                  │                               │
│                           ┌──────▼──────┐                        │
│                           │ Rate Limiter│                        │
│                           │ (sliding    │                        │
│                           │  window)    │                        │
│                           └─────────────┘                        │
└──────────────────────────────────────────────────────────────────┘
```

---

## Core Types

```rust
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify};

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Max concurrent API calls
    pub max_concurrent: usize,

    /// Max requests per rate window
    pub max_requests_per_window: u32,

    /// Rate limit window duration
    pub rate_window: Duration,

    /// Default priority for new requests
    pub default_priority: Priority,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            max_requests_per_window: 50,
            rate_window: Duration::from_secs(60),
            default_priority: Priority::Normal,
        }
    }
}

/// Request priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Priority {
    /// Inherit priority from parent record
    pub fn from_parent(parent_priority: Option<Priority>) -> Self {
        parent_priority.unwrap_or(Priority::Normal)
    }
}

/// Result of scheduling attempt
#[derive(Debug, Clone)]
pub enum ScheduleResult {
    /// Can execute immediately
    Ready,

    /// Queued, waiting for slot
    Queued {
        position: usize,
        estimated_wait: Duration,
    },

    /// Rate limited, try again later
    RateLimited {
        retry_after: Duration,
    },

    /// Request rejected (invalid, duplicate, etc.)
    Rejected {
        reason: String,
    },
}

/// A scheduled request
#[derive(Debug, Clone)]
pub struct ScheduledRequest {
    pub exec_id: String,
    pub priority: Priority,
    pub submitted_at: Instant,
    pub started_at: Option<Instant>,
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
        self.priority.cmp(&other.priority)
            .then_with(|| other.submitted_at.cmp(&self.submitted_at))
    }
}

impl PartialOrd for ScheduledRequest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
```

---

## Scheduler Implementation

```rust
pub struct Scheduler {
    config: SchedulerConfig,
    inner: Mutex<SchedulerInner>,
    notify: Notify,
}

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

#[derive(Debug, Default, Clone)]
pub struct SchedulerStats {
    pub total_scheduled: u64,
    pub total_completed: u64,
    pub total_rate_limited: u64,
    pub total_wait_time_ms: u64,
    pub peak_queue_depth: usize,
    pub peak_concurrent: usize,
}

impl Scheduler {
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
    pub async fn schedule(
        &self,
        exec_id: &str,
        priority: Priority,
    ) -> ScheduleResult {
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
        let window_start = now - self.config.rate_window;
        while inner.request_times.front().map(|t| *t < window_start).unwrap_or(false) {
            inner.request_times.pop_front();
        }

        // Check rate limit
        if inner.request_times.len() >= self.config.max_requests_per_window as usize {
            let oldest = inner.request_times.front().unwrap();
            let retry_after = self.config.rate_window - (now - *oldest);
            inner.stats.total_rate_limited += 1;
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
        let position = inner.queue.iter()
            .filter(|r| r.priority > priority || (r.priority == priority && r.submitted_at < now))
            .count() + 1;

        // Estimate wait time based on average completion time
        let avg_completion_ms = if inner.stats.total_completed > 0 {
            inner.stats.total_wait_time_ms / inner.stats.total_completed
        } else {
            30_000 // Default 30s estimate
        };

        let estimated_wait = Duration::from_millis(
            (position as u64 * avg_completion_ms) / self.config.max_concurrent as u64
        );

        ScheduleResult::Queued {
            position,
            estimated_wait,
        }
    }

    /// Wait until a slot is available for this request
    pub async fn wait_for_slot(
        &self,
        exec_id: &str,
        priority: Priority,
    ) -> Result<()> {
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
        }

        // Try to start next queued request
        if let Some(mut next) = inner.queue.pop() {
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

        let mut entries: Vec<_> = inner.running.values()
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
}

#[derive(Debug, Clone)]
pub struct QueueState {
    pub running: usize,
    pub queued: usize,
    pub rate_limited: bool,
    pub stats: SchedulerStats,
}

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub exec_id: String,
    pub priority: Priority,
    pub status: QueueEntryStatus,
    pub wait_time: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueueEntryStatus {
    Running,
    Queued,
}
```

---

## Integration with Loop Engine

```rust
impl LoopEngine {
    async fn run_iteration(&self) -> Result<IterationResult> {
        // 1. Wait for scheduler slot before making API call
        self.scheduler.wait_for_slot(&self.exec_id, self.priority).await?;

        // 2. Make API call (slot acquired)
        let result = match self.llm.complete(request).await {
            Ok(response) => {
                // 3. Release slot on success
                self.scheduler.complete(&self.exec_id).await;
                Ok(response)
            }
            Err(e) if e.is_rate_limit() => {
                // 3b. Handle rate limit
                if let LlmError::RateLimited { retry_after } = &e {
                    self.scheduler.handle_rate_limit(*retry_after).await;
                }
                self.scheduler.complete(&self.exec_id).await;
                Err(e)
            }
            Err(e) => {
                // 3c. Release slot on other errors
                self.scheduler.complete(&self.exec_id).await;
                Err(e)
            }
        }?;

        // 4. Continue with tool execution (slot released)
        self.execute_tools(&result.tool_calls).await
    }
}
```

---

## Priority Inheritance

Loops inherit priority from their parent records:

```rust
impl LoopExecution {
    pub fn priority(&self, store: &Store) -> Priority {
        // Check if parent has explicit priority
        if let Some(parent_id) = &self.parent {
            if let Ok(Some(spec)) = store.get::<Spec>(parent_id) {
                return spec.priority;
            }
            if let Ok(Some(plan)) = store.get::<Plan>(parent_id) {
                return plan.priority;
            }
        }

        // Default based on loop type
        match self.loop_type.as_str() {
            "plan" => Priority::High,      // User-facing
            "spec" => Priority::Normal,
            "phase" => Priority::Normal,
            "ralph" => Priority::Normal,
            _ => Priority::Normal,
        }
    }
}
```

---

## Configuration

From `config-schema.md`:

```yaml
concurrency:
  max-loops: 50           # Max total concurrent loops
  max-api-calls: 10       # Max concurrent LLM API calls (scheduler limit)
  max-worktrees: 50       # Max git worktrees on disk

# Rate limiting handled by scheduler internally
# Responds to 429s from Anthropic API
```

---

## TUI Integration

The TUI can display scheduler state:

```
┌─ Scheduler ──────────────────────────────────────────┐
│ Running: 10/10  Queued: 5  Rate Limited: No          │
├──────────────────────────────────────────────────────┤
│ [HIGH] exec-001  Running   2.3s                      │
│ [HIGH] exec-007  Running   1.8s                      │
│ [NORM] exec-002  Running   3.1s                      │
│ [NORM] exec-003  Running   2.9s                      │
│ [NORM] exec-004  Running   2.5s                      │
│ [NORM] exec-005  Running   2.1s                      │
│ [NORM] exec-006  Running   1.5s                      │
│ [NORM] exec-008  Running   0.9s                      │
│ [NORM] exec-009  Running   0.4s                      │
│ [NORM] exec-010  Running   0.2s                      │
│ ─────────────────────────────────────────────────────│
│ [NORM] exec-011  Queued    waiting 5.2s              │
│ [NORM] exec-012  Queued    waiting 4.8s              │
│ [NORM] exec-013  Queued    waiting 3.1s              │
│ [LOW]  exec-014  Queued    waiting 2.0s              │
│ [LOW]  exec-015  Queued    waiting 0.5s              │
├──────────────────────────────────────────────────────┤
│ Stats: 1,234 completed | 12 rate limited | avg 2.3s  │
└──────────────────────────────────────────────────────┘
```

---

## Testing

```rust
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

        // Complete one, third should get slot
        scheduler.complete("a").await;

        // Now "c" is running (was auto-promoted from queue)
        let state = scheduler.queue_state().await;
        assert_eq!(state.running, 2);
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
        let running: Vec<_> = details.iter()
            .filter(|e| e.status == QueueEntryStatus::Running)
            .collect();

        assert_eq!(running[0].exec_id, "high");
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let scheduler = Scheduler::new(SchedulerConfig {
            max_concurrent: 10,
            max_requests_per_window: 3,
            rate_window: Duration::from_secs(60),
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
}
```

---

## References

- [TaskDaemon Design](./taskdaemon-design.md) - Architecture context
- [LLM Client](./llm-client.md) - API integration
- [Config Schema](./config-schema.md) - Configuration
