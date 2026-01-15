# Design Document: Coordinator Protocol - Inter-Loop Communication

**Author:** Scott A. Idler
**Date:** 2026-01-13
**Status:** Complete
**Review Passes:** 5/5

## Summary

The Coordinator Protocol defines how concurrent agentic loops communicate and coordinate work in TaskDaemon. It provides three primitives (Alert, Query, Share) enabling event broadcasting, information requests, and data exchange between isolated loops running in separate git worktrees. The protocol uses message passing via tokio channels with persistence in TaskStore for crash recovery.

## Problem Statement

### Background

TaskDaemon orchestrates multiple concurrent agentic loops, each executing in isolation within its own git worktree. However, loops often need to coordinate:

1. **Notification:** Loop A completes a phase, loops B and C need to know
2. **Information request:** Loop A needs to know "what's the API endpoint URL?" from loop B
3. **Data sharing:** Loop A produces test results, loop B needs them for integration testing
4. **Main branch updates:** When main is updated, all loops must pause for rebase
5. **Dependency tracking:** Loop C depends on loops A and B completing first

Without coordination, loops would:
- Duplicate work (two loops implement same utility function)
- Block indefinitely (loop waits for data that never arrives)
- Create merge conflicts (both modify same file)
- Miss critical updates (rebase needed but loop continues)

### Problem

**How do we enable safe, reliable inter-loop coordination that:**
- **Preserves isolation:** Loops run independently, don't share memory
- **Survives crashes:** Messages persist across restarts
- **Handles async:** Sender doesn't block waiting for receiver
- **Prevents deadlocks:** No circular waits or dependency cycles
- **Scales gracefully:** Works with 2 loops or 50 loops
- **Stays simple:** Minimal cognitive overhead for loop authors

### Goals

- Define three coordination primitives: Alert (broadcast), Query (request/reply), Share (p2p data)
- Implement message passing via tokio mpsc channels for in-memory performance
- Persist all messages in TaskStore (coordination_events table) for durability
- Support async non-blocking sends (don't wait for receiver to process)
- Provide timeout mechanisms (queries don't wait forever)
- Enable proactive rebase notifications (main branch update alerts)
- Detect circular dependencies before spawning loops

### Non-Goals

- **Not a distributed system:** All loops run on same machine, same process
- **Not a queue system:** No message ordering guarantees beyond FIFO per-sender
- **Not a pub/sub broker:** No topic subscriptions, no message routing beyond direct addressing
- **Not transactional:** Messages are fire-and-forget, no two-phase commit
- **Not consensus:** No leader election, no quorum requirements
- **Not synchronous RPC:** All communication is async message passing

## Proposed Solution

### Overview

The Coordinator is a tokio task that mediates all inter-loop communication. It maintains:

1. **Registry:** Map of execution ID → channel sender for routing messages
2. **Pending queries:** Map of query ID → oneshot reply channel
3. **Subscription lists:** Map of event type → list of interested execution IDs

Loops interact with Coordinator via:
- **alert(event):** Broadcast event to all subscribers
- **query(target, question, timeout):** Send question, await reply with timeout
- **share(target, data):** Send data to specific loop
- **subscribe(event_type):** Register interest in event type
- **stop(target, reason):** Request loop termination

All operations are async and non-blocking. The Coordinator persists messages to TaskStore for crash recovery.

### Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                      TaskDaemon Process                       │
│                                                               │
│  ┌────────────┐    ┌────────────┐    ┌────────────┐        │
│  │  Loop A    │    │  Loop B    │    │  Loop C    │        │
│  │ (exec-001) │    │ (exec-002) │    │ (exec-003) │        │
│  └──────┬─────┘    └──────┬─────┘    └──────┬─────┘        │
│         │                  │                  │               │
│         │alert/query      │query/share       │subscribe      │
│         │                  │                  │               │
│         ▼                  ▼                  ▼               │
│  ┌─────────────────────────────────────────────────┐        │
│  │            Coordinator Task                      │        │
│  │  ┌─────────────────────────────────────────┐   │        │
│  │  │  Registry:                               │   │        │
│  │  │    exec-001 → Sender<CoordMessage>      │   │        │
│  │  │    exec-002 → Sender<CoordMessage>      │   │        │
│  │  │    exec-003 → Sender<CoordMessage>      │   │        │
│  │  └─────────────────────────────────────────┘   │        │
│  │  ┌─────────────────────────────────────────┐   │        │
│  │  │  Pending Queries:                        │   │        │
│  │  │    query-abc → Sender<QueryReply>       │   │        │
│  │  └─────────────────────────────────────────┘   │        │
│  │  ┌─────────────────────────────────────────┐   │        │
│  │  │  Subscriptions:                          │   │        │
│  │  │    "phase_complete" → [exec-001, ...]   │   │        │
│  │  │    "main_updated" → [exec-002, ...]     │   │        │
│  │  └─────────────────────────────────────────┘   │        │
│  └───────────────────┬─────────────────────────────┘        │
│                      │ persist                               │
│                      ▼                                        │
│  ┌──────────────────────────────────────────────────┐       │
│  │              TaskStore                            │       │
│  │  (coordination_events table: alert/query/share records)│       │
│  └──────────────────────────────────────────────────┘       │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

### Data Model

Coordination messages are stored in the `dependencies` table in TaskStore:

```sql
CREATE TABLE dependencies (
    id TEXT PRIMARY KEY,              -- UUIDv7 (sortable)
    from_exec_id TEXT NOT NULL,       -- Sender execution ID
    to_exec_id TEXT,                  -- Receiver execution ID, NULL for broadcast
    dependency_type TEXT NOT NULL,    -- 'alert' | 'query' | 'share'
    created_at INTEGER NOT NULL,
    resolved_at INTEGER,              -- NULL if pending, timestamp if resolved
    payload TEXT,                     -- JSON payload
    FOREIGN KEY (from_exec_id) REFERENCES executions(id) ON DELETE CASCADE,
    FOREIGN KEY (to_exec_id) REFERENCES executions(id) ON DELETE CASCADE
);
```

**Note:** For broadcast notifications, `to_exec_id` is NULL (not a specific execution). The FK constraint is nullable to allow this.

**Alert payload:**
```json
{
  "event-type": "phase_complete",
  "data": {
    "phase-name": "Phase 1: Core Logic",
    "commit-sha": "abc123..."
  }
}
```

**Query payload:**
```json
{
  "query-id": "550e8400-...",
  "question": "What is the API base URL?",
  "timeout-ms": 30000
}
```

**Query reply (in resolved payload):**
```json
{
  "query-id": "550e8400-...",
  "answer": "http://localhost:8080/api/v1",
  "replied-at": 1704074400000
}
```

**Share payload:**
```json
{
  "share-type": "test_results",
  "data": {
    "passed": 42,
    "failed": 3,
    "log-file": "/path/to/test.log"
  }
}
```

### API Design

```rust
use eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use std::time::Duration;

/// Handle for interacting with Coordinator
#[derive(Clone)]
pub struct CoordinatorHandle {
    tx: mpsc::Sender<CoordRequest>,
}

impl CoordinatorHandle {
    /// Broadcast event to all subscribers
    pub async fn alert(&self, event_type: &str, data: serde_json::Value) -> Result<()>;

    /// Send question to specific execution, await reply with timeout
    pub async fn query(
        &self,
        target_exec_id: &str,
        question: &str,
        timeout: Duration
    ) -> Result<String>;

    /// Share data with specific execution
    pub async fn share(
        &self,
        target_exec_id: &str,
        share_type: &str,
        data: serde_json::Value
    ) -> Result<()>;

    /// Subscribe to event type (receive notifications)
    pub async fn subscribe(&self, event_type: &str) -> Result<()>;

    /// Request execution to stop gracefully
    pub async fn stop(&self, target_exec_id: &str, reason: &str) -> Result<()>;

    /// Receive messages (notifications, queries, shares, stop requests)
    pub async fn recv(&mut self) -> Option<CoordMessage>;

    /// Reply to a query (called by receiver)
    pub async fn reply_query(&self, query_id: &str, answer: &str) -> Result<()>;
}

/// Messages sent to loops
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordMessage {
    Notification {
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        #[serde(rename = "event-type")]
        event_type: String,
        data: serde_json::Value,
    },
    Query {
        #[serde(rename = "query-id")]
        query_id: String,
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        question: String,
    },
    Share {
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        #[serde(rename = "share-type")]
        share_type: String,
        data: serde_json::Value,
    },
    Stop {
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        reason: String,
    },
}

/// Internal requests to Coordinator
enum CoordRequest {
    Register {
        exec_id: String,
        tx: mpsc::Sender<CoordMessage>,
    },
    Unregister {
        exec_id: String,
    },
    Alert {
        from_exec_id: String,
        event_type: String,
        data: serde_json::Value,
    },
    Query {
        query_id: String,
        from_exec_id: String,
        target_exec_id: String,
        question: String,
        reply_tx: oneshot::Sender<Result<String>>,
        timeout: Duration,
    },
    QueryReply {
        query_id: String,
        answer: String,
    },
    Share {
        from_exec_id: String,
        target_exec_id: String,
        share_type: String,
        data: serde_json::Value,
    },
    Subscribe {
        exec_id: String,
        event_type: String,
    },
    Stop {
        from_exec_id: String,
        target_exec_id: String,
        reason: String,
    },
    QueryTimeout {
        query_id: String,
    },
}
```

### Usage Examples

**Loop subscribing to events and receiving notification:**
```rust
// In loop task
let mut coord = coordinator_handle.clone();

// Subscribe to phase completions
coord.subscribe("phase_complete").await?;

// Main loop
loop {
    tokio::select! {
        // Handle coordinator messages
        Some(msg) = coord.recv() => {
            match msg {
                CoordMessage::Notification { from_exec_id, event_type, data } => {
                    tracing::info!("Received {} from {}", event_type, from_exec_id);
                    // React to notification
                }
                CoordMessage::Query { query_id, question, .. } => {
                    let answer = process_question(&question)?;
                    coord.reply_query(&query_id, &answer).await?;
                }
                _ => {}
            }
        }

        // Do work
        work_result = do_phase_work() => {
            // Alert others when phase completes
            coord.alert("phase_complete", json!({
                "phase": "Phase 1",
                "commit": "abc123"
            })).await?;
        }
    }
}
```

**Loop querying another loop:**
```rust
// Loop A needs info from Loop B
let api_url = coord.query(
    "exec-002",  // Loop B's execution ID
    "What is the API base URL?",
    Duration::from_secs(30)
).await?;

tracing::info!("Got API URL from Loop B: {}", api_url);
```

**Loop sharing data with another loop:**
```rust
// Loop A shares test results with Loop B
coord.share(
    "exec-002",
    "test_results",
    json!({
        "passed": 42,
        "failed": 3,
        "log-path": "/tmp/test.log"
    })
).await?;
```

### Coordinator Task Implementation

**Main event loop:**
```rust
async fn run_coordinator(
    coord_tx: mpsc::Sender<CoordRequest>,  // Clone of sender for timeout spawns
    mut rx: mpsc::Receiver<CoordRequest>,
    store_tx: mpsc::Sender<StoreMessage>,  // Send messages to state manager
) {
    let mut registry: HashMap<String, mpsc::Sender<CoordMessage>> = HashMap::new();
    let mut subscriptions: HashMap<String, Vec<String>> = HashMap::new();
    let mut pending_queries: HashMap<String, oneshot::Sender<Result<String>>> = HashMap::new();

    while let Some(req) = rx.recv().await {
        match req {
            CoordRequest::Register { exec_id, tx } => {
                registry.insert(exec_id.clone(), tx);
                tracing::info!("Registered execution: {}", exec_id);
            }

            CoordRequest::Unregister { exec_id } => {
                registry.remove(&exec_id);
                tracing::info!("Unregistered execution: {}", exec_id);
            }

            CoordRequest::Alert { from_exec_id, event_type, data } => {
                // Persist to store via state manager (fire-and-forget)
                let dep = Dependency {
                    id: uuid::Uuid::now_v7().to_string(),
                    from_exec_id: from_exec_id.clone(),
                    to_exec_id: None,  // NULL for broadcast
                    dependency_type: DependencyType::Alert,
                    created_at: now_ms(),
                    resolved_at: None,
                    payload: Some(json!({ "event-type": event_type, "data": data }).to_string()),
                };
                let _ = store_tx.send(StoreMessage::CreateDependency(dep)).await;

                // Broadcast to subscribers
                if let Some(subscribers) = subscriptions.get(&event_type) {
                    for exec_id in subscribers {
                        if let Some(tx) = registry.get(exec_id) {
                            let msg = CoordMessage::Notification {
                                from_exec_id: from_exec_id.clone(),
                                event_type: event_type.clone(),
                                data: data.clone(),
                            };
                            let _ = tx.send(msg).await;  // Ignore send errors
                        }
                    }
                }
            }

            CoordRequest::Query { query_id, from_exec_id, target_exec_id, question, reply_tx, timeout } => {
                // Persist query to store via state manager
                let dep = Dependency {
                    id: query_id.clone(),
                    from_exec_id: from_exec_id.clone(),
                    to_exec_id: Some(target_exec_id.clone()),
                    dependency_type: DependencyType::Query,
                    created_at: now_ms(),
                    resolved_at: None,
                    payload: Some(json!({ "query-id": &query_id, "question": &question }).to_string()),
                };
                let _ = store_tx.send(StoreMessage::CreateDependency(dep)).await;

                // Send query to target
                if let Some(tx) = registry.get(&target_exec_id) {
                    let msg = CoordMessage::Query {
                        query_id: query_id.clone(),
                        from_exec_id,
                        question,
                    };
                    let _ = tx.send(msg).await;

                    // Track pending query
                    pending_queries.insert(query_id.clone(), reply_tx);

                    // Spawn timeout handler (coord_tx passed as param, not rx.sender())
                    let query_id_clone = query_id.clone();
                    let timeout_tx = coord_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(timeout).await;
                        // Send timeout message to coordinator
                        let _ = timeout_tx.send(CoordRequest::QueryTimeout { query_id: query_id_clone }).await;
                    });
                } else {
                    let _ = reply_tx.send(Err(eyre!("Target execution not found")));
                }
            }

            CoordRequest::QueryTimeout { query_id } => {
                // Clean up timed-out query
                if let Some(reply_tx) = pending_queries.remove(&query_id) {
                    let _ = reply_tx.send(Err(eyre!("Query timeout")));
                    tracing::warn!("Query {} timed out", query_id);
                }
            }

            CoordRequest::QueryReply { query_id, answer } => {
                // Update store with reply
                store.resolve_dependency(&query_id, Some(answer.clone())).await?;

                // Send reply to waiting sender
                if let Some(reply_tx) = pending_queries.remove(&query_id) {
                    reply_tx.send(Ok(answer)).ok();
                }
            }

            // ... other request types
        }
    }
}
```

### Edge Cases and Error Handling

**Query timeout race condition:**
- If reply arrives just as timeout fires, both will try to remove from pending_queries
- Solution: QueryReply checks if query exists before removing, timeout also checks

**Loop crash detection:**
- If loop crashes, its sender is dropped, sending fails
- Solution: On send error, automatically unregister the execution

**Subscription persistence:**
- Subscriptions are in-memory only, lost on Coordinator restart
- Solution: Loops must re-subscribe after Coordinator restart
- Ralph loops include subscribe calls in their initialization phase

**Message ordering:**
- No cross-sender ordering guarantee (FIFO per sender only)
- Solution: Document limitation, use sequence numbers in payload if ordering matters

**Coordinator supervision:**
- If Coordinator crashes, TaskDaemon should restart it
- Solution: Wrap Coordinator task in supervisor that restarts on panic

```rust
loop {
    let result = tokio::spawn(run_coordinator(...)).await;
    match result {
        Err(e) => {
            tracing::error!("Coordinator crashed: {:?}, restarting...", e);
            tokio::time::sleep(Duration::from_secs(1)).await;
            // Restart coordinator
        }
        Ok(_) => break,  // Clean shutdown
    }
}
```

### Crash Recovery Mechanism

**On Coordinator startup:**
1. Read all unresolved dependencies from TaskStore where `resolved-at` is NULL
2. For pending queries:
   - If both sender and receiver executions are still running, resend query
   - Otherwise, mark as failed (timeout)
3. For pending notifications:
   - Rebroadcast to current subscribers
4. For pending shares:
   - Resend to target if still registered

**Implementation:**
```rust
async fn recover_pending_messages(store: &Store, registry: &HashMap<String, Sender>) -> Result<()> {
    let pending = store.list_dependencies_unresolved().await?;

    for dep in pending {
        match dep.dependency_type {
            DependencyType::Query => {
                // Check if both parties still exist
                if registry.contains_key(&dep.from_exec_id) && registry.contains_key(&dep.to_exec_id) {
                    // Resend query
                    let payload: QueryPayload = serde_json::from_str(&dep.payload.unwrap())?;
                    // ... resend query message
                } else {
                    // Mark as failed
                    store.resolve_dependency(&dep.id, Some("timeout: execution not found".to_string())).await?;
                }
            }
            DependencyType::Alert => {
                // Rebroadcast notification
                let payload: AlertPayload = serde_json::from_str(&dep.payload.unwrap())?;
                // ... rebroadcast to current subscribers
            }
            // ... other types
        }
    }

    Ok(())
}
```

### Circular Dependency Detection

**Algorithm (Tarjan's SCC):**
```rust
fn detect_circular_dependencies(task_specs: &[TaskSpec]) -> Result<Vec<Vec<String>>> {
    // Build dependency graph
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    for ts in task_specs {
        // Parse dependencies from TS content or metadata
        let deps = extract_dependencies(ts)?;
        graph.insert(ts.id.clone(), deps);
    }

    // Run Tarjan's algorithm to find strongly connected components
    let mut index = 0;
    let mut stack = Vec::new();
    let mut indices: HashMap<String, usize> = HashMap::new();
    let mut lowlinks: HashMap<String, usize> = HashMap::new();
    let mut on_stack: HashSet<String> = HashSet::new();
    let mut sccs: Vec<Vec<String>> = Vec::new();

    for node in graph.keys() {
        if !indices.contains_key(node) {
            strongconnect(node, &graph, &mut index, &mut stack, &mut indices,
                         &mut lowlinks, &mut on_stack, &mut sccs);
        }
    }

    // Filter out single-node components (not cycles)
    let cycles: Vec<Vec<String>> = sccs.into_iter()
        .filter(|scc| scc.len() > 1)
        .collect();

    if !cycles.is_empty() {
        Err(eyre!("Circular dependencies detected: {:?}", cycles))
    } else {
        Ok(Vec::new())
    }
}
```

### Rate Limiting Implementation

**Per-loop message counter with sliding window:**
```rust
struct RateLimiter {
    counters: HashMap<String, VecDeque<Instant>>,
    limit: usize,
    window: Duration,
}

impl RateLimiter {
    fn check_and_record(&mut self, exec_id: &str) -> bool {
        let now = Instant::now();
        let counter = self.counters.entry(exec_id.to_string()).or_insert_with(VecDeque::new);

        // Remove timestamps outside window
        while let Some(&timestamp) = counter.front() {
            if now.duration_since(timestamp) > self.window {
                counter.pop_front();
            } else {
                break;
            }
        }

        // Check if under limit
        if counter.len() < self.limit {
            counter.push_back(now);
            true
        } else {
            tracing::warn!("Rate limit exceeded for execution: {}", exec_id);
            false
        }
    }
}

// In Coordinator task
let mut rate_limiter = RateLimiter::new(100, Duration::from_secs(1));

match req {
    CoordRequest::Alert { from_exec_id, .. } => {
        if !rate_limiter.check_and_record(&from_exec_id) {
            continue;  // Drop message
        }
        // ... process alert
    }
    // ... other requests
}
```

### Implementation Plan

#### Phase 1: Core Coordinator Task
- Implement Coordinator tokio task with message loop
- Add registry (HashMap<ExecId, Sender>)
- Add subscription tracking (HashMap<EventType, Vec<ExecId>>)
- Implement register/unregister for loop lifecycle

#### Phase 2: Alert Primitive
- Implement alert() broadcasting to subscribers
- Persist notifications to TaskStore coordination_events table
- Add subscribe() for event registration
- Test: Loop A notifies, loops B and C receive

#### Phase 3: Query Primitive
- Implement query() with timeout using tokio::time::timeout
- Track pending queries (HashMap<QueryId, Sender<Reply>>)
- Implement query reply handling
- Persist queries and replies to TaskStore
- Test: Loop A queries loop B, receives answer within timeout

#### Phase 4: Share Primitive
- Implement share() for point-to-point data transfer
- Persist shares to TaskStore
- Test: Loop A shares test results with loop B

#### Phase 5: Stop Primitive
- Implement stop() for graceful termination requests
- Loops handle Stop messages by cleaning up and exiting
- Test: Loop A requests loop B to stop, B completes current work and exits

#### Phase 6: Proactive Rebase Integration
- Special event type: "main_updated"
- TaskDaemon polls git, detects main branch updates
- Coordinator broadcasts "main_updated" notification to all loops
- Loops pause, rebase, resume

#### Phase 7: Circular Dependency Detection
- Before spawning loops, analyze TS dependencies
- Build dependency graph
- Run cycle detection algorithm (DFS-based)
- Fail fast if cycle detected, report to user

## Alternatives Considered

### Alternative 1: Shared Memory + Locks
- **Description:** Loops share Arc<Mutex<State>>, coordinate via locks
- **Pros:** Fast, no serialization overhead
- **Cons:** Deadlock risk, no crash recovery, tight coupling
- **Why not chosen:** TaskDaemon uses actor pattern with message passing, not shared state

### Alternative 2: File-Based Communication
- **Description:** Loops write messages to .taskstore/messages/ directory
- **Pros:** Survives crashes, simple implementation
- **Cons:** Slow (file I/O per message), no async support, polling overhead
- **Why not chosen:** Performance unacceptable for real-time coordination

### Alternative 3: External Message Broker (Redis, RabbitMQ)
- **Description:** Use external message queue for coordination
- **Pros:** Battle-tested, rich features (pub/sub, persistence, routing)
- **Cons:** External dependency, overkill for single-process, setup complexity
- **Why not chosen:** TaskDaemon is self-contained, no external services

### Alternative 4: Direct Loop-to-Loop Channels
- **Description:** Each loop has direct channels to every other loop
- **Pros:** No mediator overhead
- **Cons:** O(N²) channels, no central persistence, no subscription model
- **Why not chosen:** Doesn't scale, harder to implement broadcast/subscribe

### Alternative 5: Unix Domain Sockets
- **Description:** Loops communicate via UDS
- **Pros:** Language-agnostic, survives process crashes
- **Cons:** Loops run in same process (no separate processes), serialization overhead
- **Why not chosen:** Loops are tokio tasks, not separate processes

## Technical Considerations

### Dependencies

**Rust crates:**
- `tokio` (mpsc channels, timeouts)
- `serde` + `serde_json` (message serialization)
- `uuid` (query IDs, message IDs)
- `tracing` (logging)

**Internal dependencies:**
- `taskstore` (persist messages)

### Performance

**Expected characteristics:**
- **Alert latency:** <1ms (in-memory channel send)
- **Query round-trip:** <10ms (two channel hops + processing)
- **Share latency:** <1ms (in-memory send)
- **Throughput:** 10K messages/sec (tokio mpsc benchmarks)

**Scale limits:**
- Max 100 concurrent executions (registry size)
- Max 1000 pending queries at once
- Max 100 event types with subscriptions

**Optimization strategies:**
- Use unbounded channels for notifications (fire-and-forget)
- Use bounded channels for queries (backpressure if overwhelmed)
- Batch persist to TaskStore (write N messages per transaction)

### Observability and Debugging

**Coordinator metrics:**
```rust
pub struct CoordinatorMetrics {
    pub registered_executions: usize,
    pub pending_queries: usize,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub query_timeouts: u64,
    pub rate_limit_violations: u64,
}
```

**Message tracing:**
- Every message gets unique ID (UUIDv7)
- Log at INFO level: sent, delivered, replied
- Use tracing spans for correlation:

```rust
let span = tracing::info_span!("query", query_id = %query_id, from = %from_exec_id, to = %target_exec_id);
let _enter = span.enter();
tracing::info!("Sending query");
// ... send logic
tracing::info!("Query sent");
```

**TUI Integration:**
- Coordinator exposes `subscribe_to_all_events()` for TUI
- TUI displays recent coordination events in a log panel
- TUI shows pending queries count in status bar

### Integration with Other Components

**With TaskStore:**
- Coordinator sends messages to state manager actor
- State manager persists dependencies asynchronously
- No blocking waits for persistence

**With Loop Executor:**
- Each loop receives CoordinatorHandle on spawn
- Loop's main select! loop includes `coord.recv()` arm
- Loop can send alert/query/share at any point in its workflow

**With Ralph Loop Engine:**
- Loops call CoordinatorHandle methods directly in Rust
- Each iteration can send alerts, queries, or shares as needed
- Example usage in loop:
```rust
// Alert on phase completion
coord.alert("phase_complete", json!({
    "phase": phase_name,
    "commit": commit_sha,
})).await?;

// Query another loop for information
let api_url = coord.query(
    &dependency_exec_id,
    "What is the API URL?",
    Duration::from_secs(30)
).await?;
```

### Security

**Threat model:**
- All loops run within same process, trust boundary is process
- No authentication/authorization needed between loops
- Malicious loop could DoS Coordinator with message spam

**Mitigations:**
- Bounded channels prevent unbounded memory growth
- Rate limiting: Max 100 messages/sec per loop (drop excess)
- Query timeout prevents indefinite waits

### Testing Strategy

**Unit tests:**
- Test Coordinator registration/unregistration
- Test alert() broadcasts to all subscribers
- Test query() with timeout (success and timeout cases)
- Test share() point-to-point delivery
- Test stop() handling

**Integration tests:**
- Spawn 3 real loops, test alert/query/share coordination
- Simulate crash: Kill Coordinator, restart, verify state restored from TaskStore
- Test circular dependency detection
- Test proactive rebase notification flow

**Load tests:**
- 50 loops, 1000 messages/sec, measure latency and throughput
- Test query timeout under load

### Rollout Plan

**Phase 1: Coordinator implementation**
- Build Coordinator task with alert/query/share primitives
- Test in isolation with mock loops

**Phase 2: TaskDaemon integration**
- Spawn Coordinator at TaskDaemon startup
- Loops receive CoordinatorHandle on spawn
- Integrate with loop lifecycle (register on spawn, unregister on exit)

**Phase 3: Ralph loop integration**
- Integrate coordination into Ralph loop execution engine
- Add CoordinatorHandle to loop context
- Test loops that use coordination primitives

**Phase 4: Proactive rebase**
- Implement main branch polling
- Broadcast "main_updated" notifications
- Loops handle rebase flow

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Coordinator becomes bottleneck | Low | High | Profile message throughput, optimize if needed, consider sharding by exec ID |
| Query timeout too short | Medium | Medium | Make timeout configurable per-query, default 30s, loop type config can override |
| Message loss on crash | Low | High | Persist all messages to TaskStore before sending, replay on restart |
| Circular dependencies undetected | Low | High | Implement cycle detection before spawning loops, fail fast |
| Deadlock (loop A queries B, B queries A) | Medium | High | Document: Queries should not trigger other queries, use timeouts |
| Notification spam | Medium | Low | Rate limit per-loop (100 msg/sec), log violations, optionally pause spammer |
| Query reply never arrives | Medium | Medium | Always use timeout, default 30s, return error on timeout |

## Open Questions

- [ ] Should we support filtered subscriptions (e.g., "phase_complete for exec-001 only")?
- [ ] Should query replies be cached (same question asked twice)?
- [ ] Should we support message priorities (urgent vs normal)?
- [ ] Should we add a "cancel query" operation?
- [ ] Should we limit payload size (e.g., max 1MB per message)?

## References

- [Main Design](./taskdaemon-design.md) - Overall architecture
- [Execution Model](./execution-model-design.md) - Git worktree management
- [Implementation Details](./implementation-details.md) - Loop schema, domain types
- [Config Schema](./config-schema.md) - Configuration hierarchy
- [TaskStore](https://github.com/saidler/taskstore) - Persistent state library
- [Tokio mpsc channels](https://docs.rs/tokio/latest/tokio/sync/mpsc/)
- [Actor pattern in Rust](https://ryhl.io/blog/actors-with-tokio/)
