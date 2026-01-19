# Spec: Execution State Tracking

**ID:** 009-execution-tracking  
**Status:** Draft  
**Dependencies:** [006-domain-types, 007-state-manager-actor]

## Summary

Implement comprehensive execution state tracking that monitors the lifecycle of loop executions, captures runtime metrics, and provides visibility into the current state of all running and completed loops.

## Acceptance Criteria

1. **Execution Lifecycle**
   - Track execution from creation to completion
   - State transitions with timestamps
   - Parent-child execution relationships
   - Resource usage tracking

2. **Runtime Metrics**
   - Iteration count and duration
   - Token usage statistics
   - Tool invocation metrics
   - Error and retry counts

3. **State Persistence**
   - Real-time state updates
   - Execution history preservation
   - Query capabilities
   - Archival policies

4. **Observability**
   - Live execution monitoring
   - Historical analysis
   - Performance insights
   - Anomaly detection

## Implementation Phases

### Phase 1: Execution Model
- Define execution state types
- Create state machine
- Implement transitions
- Add validation rules

### Phase 2: Metrics Collection
- Token usage tracking
- Timing instrumentation
- Resource monitoring
- Error aggregation

### Phase 3: Persistence Layer
- Real-time updates
- History management
- Query optimization
- Archival system

### Phase 4: Analytics
- Metric aggregation
- Trend analysis
- Anomaly detection
- Reporting tools

## Technical Details

### Module Structure
```
src/execution/
├── mod.rs
├── state.rs       # Execution state machine
├── tracker.rs     # State tracker
├── metrics.rs     # Metrics collection
├── history.rs     # Historical queries
└── analytics.rs   # Analytics engine
```

### Execution State
```rust
pub struct LoopExecution {
    pub id: Uuid,
    pub loop_type: LoopType,
    pub target_id: Uuid,  // Plan/Spec/Phase ID
    pub parent_execution_id: Option<Uuid>,
    pub status: ExecutionStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub iteration_count: u32,
    pub metrics: ExecutionMetrics,
    pub error: Option<ExecutionError>,
}

pub struct ExecutionMetrics {
    pub total_tokens: TokenUsage,
    pub tool_invocations: HashMap<String, u32>,
    pub iteration_durations: Vec<Duration>,
    pub retry_count: u32,
    pub resource_usage: ResourceMetrics,
}

pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_cost: Option<Decimal>,
}
```

### State Transitions
```
Created → Queued → Running → [Completed|Failed|Cancelled]
                 ↓
              Paused → Running
```

### Tracking Features
1. **Real-time Updates**: WebSocket or SSE for live monitoring
2. **Historical Queries**: Time-range based analysis
3. **Parent-Child Trees**: Execution hierarchy visualization
4. **Resource Tracking**: CPU, memory, disk usage per execution

## Notes

- Consider implementing execution replay for debugging
- Metrics should be designed for both real-time monitoring and historical analysis
- Implement data retention policies to manage storage growth
- Provide aggregated views for high-level system health monitoring