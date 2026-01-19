# Spec: Coordinator Message Routing

**ID:** 010-coordinator-routing  
**Status:** Draft  
**Dependencies:** [003-loop-engine-core]

## Summary

Create the Coordinator component that handles message routing between loops using a typed message system (Alert, Query, Share, Stop). This enables loops to communicate, share state, and coordinate their activities.

## Acceptance Criteria

1. **Message Types**
   - Alert: Notify other loops of important events
   - Query: Request information from other loops
   - Share: Broadcast state updates
   - Stop: Gracefully terminate loops

2. **Routing Logic**
   - Topic-based routing
   - Direct loop-to-loop messaging
   - Broadcast capabilities
   - Message filtering

3. **Delivery Guarantees**
   - At-least-once delivery
   - Message ordering per sender
   - Timeout handling
   - Dead letter queue

4. **Performance**
   - Low-latency routing
   - High throughput support
   - Backpressure handling
   - Efficient serialization

## Implementation Phases

### Phase 1: Core Message System
- Define message types
- Create coordinator actor
- Implement basic routing
- Add message validation

### Phase 2: Routing Engine
- Topic subscription system
- Direct messaging
- Broadcast mechanisms
- Message filtering

### Phase 3: Reliability
- Delivery tracking
- Retry logic
- Timeout handling
- Dead letter queue

### Phase 4: Advanced Features
- Message prioritization
- Rate limiting
- Metrics collection
- Debug tooling

## Technical Details

### Module Structure
```
src/coordination/
├── mod.rs
├── coordinator.rs # Main coordinator
├── messages.rs    # Message definitions
├── routing.rs     # Routing engine
├── delivery.rs    # Delivery guarantees
└── topics.rs      # Topic management
```

### Message Types
```rust
pub enum CoordinationMessage {
    Alert {
        id: Uuid,
        from: LoopId,
        severity: AlertSeverity,
        message: String,
        context: Value,
    },
    Query {
        id: Uuid,
        from: LoopId,
        to: QueryTarget,
        query: QueryType,
        timeout: Duration,
    },
    Share {
        id: Uuid,
        from: LoopId,
        topic: String,
        data: Value,
        ttl: Option<Duration>,
    },
    Stop {
        id: Uuid,
        target: StopTarget,
        reason: String,
        grace_period: Duration,
    },
}

pub enum QueryTarget {
    Loop(LoopId),
    Topic(String),
    Broadcast,
}

pub struct RoutingTable {
    topics: HashMap<String, HashSet<LoopId>>,
    loops: HashMap<LoopId, LoopInfo>,
    pending_queries: HashMap<Uuid, PendingQuery>,
}
```

### Routing Rules
1. **Direct**: Route to specific loop by ID
2. **Topic**: Route to all subscribers of a topic
3. **Broadcast**: Route to all active loops
4. **Filtered**: Apply predicates before delivery

### Delivery Mechanism
```rust
pub struct MessageDelivery {
    pub message: CoordinationMessage,
    pub attempts: u32,
    pub created_at: DateTime<Utc>,
    pub next_retry: Option<DateTime<Utc>>,
    pub acknowledgments: HashSet<LoopId>,
}
```

## Notes

- Messages should be schema-validated to prevent corruption
- Consider implementing message compression for large payloads
- Dead letter queue should have monitoring and alerting
- Provide message tracing for debugging complex interactions