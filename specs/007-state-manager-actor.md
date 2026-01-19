# Spec: State Manager Actor

**ID:** 007-state-manager-actor  
**Status:** Draft  
**Dependencies:** [006-domain-types]

## Summary

Build the `StateManager` as an actor-based system for managing persistent state with JSONL storage. The actor model ensures thread-safe state mutations and provides a clean API for state queries and updates.

## Acceptance Criteria

1. **Actor Implementation**
   - Message-based communication
   - Async message handling
   - Thread-safe state mutations
   - Graceful shutdown

2. **Storage Backend**
   - JSONL append-only format
   - Atomic writes
   - Periodic compaction
   - Fast startup scanning

3. **Message Types**
   - CRUD operations for all domain types
   - Bulk queries with filtering
   - Transaction support
   - State snapshots

4. **Performance**
   - Sub-millisecond response for queries
   - Efficient indexing
   - Lazy loading strategies
   - Memory usage bounds

## Implementation Phases

### Phase 1: Actor Infrastructure
- Create actor framework
- Define message types
- Implement message routing
- Add supervision logic

### Phase 2: Storage Layer
- JSONL file management
- Append-only writes
- File rotation logic
- Corruption recovery

### Phase 3: State Operations
- CRUD implementations
- Query engine
- Index management
- Cache layer

### Phase 4: Advanced Features
- Transaction support
- Bulk operations
- State snapshots
- Performance monitoring

## Technical Details

### Module Structure
```
src/store/state_manager/
├── mod.rs
├── actor.rs       # Actor implementation
├── messages.rs    # Message definitions
├── storage.rs     # JSONL storage backend
├── indices.rs     # In-memory indices
├── queries.rs     # Query engine
└── compaction.rs  # Log compaction
```

### Actor Messages
```rust
pub enum StateMessage {
    // Plan operations
    CreatePlan(Plan, oneshot::Sender<Result<Plan, Error>>),
    GetPlan(Uuid, oneshot::Sender<Result<Option<Plan>, Error>>),
    UpdatePlan(Uuid, PlanUpdate, oneshot::Sender<Result<Plan, Error>>),
    
    // Spec operations
    CreateSpec(Spec, oneshot::Sender<Result<Spec, Error>>),
    GetSpecsForPlan(Uuid, oneshot::Sender<Result<Vec<Spec>, Error>>),
    
    // Query operations
    Query(StateQuery, oneshot::Sender<Result<QueryResult, Error>>),
    
    // Maintenance
    Compact(oneshot::Sender<Result<CompactionStats, Error>>),
    Snapshot(oneshot::Sender<Result<StateSnapshot, Error>>),
}
```

### Storage Format
```jsonl
{"type":"plan","timestamp":"2024-01-20T10:00:00Z","data":{"id":"...","name":"..."}}
{"type":"spec","timestamp":"2024-01-20T10:01:00Z","data":{"id":"...","plan_id":"..."}}
{"type":"update","timestamp":"2024-01-20T10:02:00Z","target":"plan","id":"...","changes":{}}
```

### Indexing Strategy
- Primary indices on IDs
- Secondary indices on relationships
- Status-based indices for queries
- Time-based indices for history

## Notes

- The actor should never panic; all errors should be returned
- Consider implementing read replicas for scalability
- JSONL format allows for easy debugging and recovery
- Implement graceful degradation if indices become corrupted