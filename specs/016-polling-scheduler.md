# Spec: Polling Scheduler

**ID:** 016-polling-scheduler  
**Status:** Draft  
**Dependencies:** [011-priority-scheduler, 014-loop-manager]

## Summary

Implement a polling scheduler that periodically checks for ready Specs and schedules them for execution. The scheduler should efficiently identify work that's ready to run based on dependencies and resource availability.

## Acceptance Criteria

1. **Polling Mechanism**
   - Configurable polling interval
   - Efficient ready-state checking
   - Batched scheduling decisions
   - Jitter to prevent thundering herd

2. **Ready Detection**
   - Dependency satisfaction checking
   - Resource availability verification
   - Priority consideration
   - State validation

3. **Scheduling Logic**
   - Batch scheduling for efficiency
   - Fair distribution across loop types
   - Respect for priorities
   - Resource allocation

4. **Performance**
   - Minimal database queries
   - Cached dependency state
   - Incremental updates
   - Low overhead operation

## Implementation Phases

### Phase 1: Polling Infrastructure
- Timer-based polling loop
- Configuration system
- Basic ready detection
- Scheduling interface

### Phase 2: Ready State Logic
- Dependency checking
- Resource verification
- Priority handling
- Batch collection

### Phase 3: Optimization
- State caching
- Incremental updates
- Query optimization
- Batch processing

### Phase 4: Monitoring
- Polling metrics
- Scheduling latency
- Ready queue depth
- Performance tracking

## Technical Details

### Module Structure
```
src/scheduling/polling/
├── mod.rs
├── poller.rs      # Main polling loop
├── ready.rs       # Ready state detection
├── batch.rs       # Batch scheduling
├── cache.rs       # State caching
└── metrics.rs     # Performance metrics
```

### Core Types
```rust
pub struct PollingScheduler {
    interval: Duration,
    state_manager: Arc<StateManager>,
    scheduler: Arc<Scheduler>,
    loop_manager: Arc<LoopManager>,
    ready_cache: ReadyStateCache,
    metrics: SchedulerMetrics,
}

pub struct ReadyStateCache {
    dependency_status: HashMap<Uuid, DependencyStatus>,
    resource_availability: ResourceState,
    last_update: Instant,
    ttl: Duration,
}

pub struct SchedulingBatch {
    pub ready_specs: Vec<ReadySpec>,
    pub scheduling_time: DateTime<Utc>,
    pub resource_snapshot: ResourceState,
}

pub struct ReadySpec {
    pub spec: Spec,
    pub priority: Priority,
    pub dependencies_met: bool,
    pub resources_available: bool,
    pub waiting_time: Duration,
}
```

### Polling Algorithm
```rust
impl PollingScheduler {
    pub async fn run(&mut self) {
        let mut interval = tokio::time::interval(self.interval);
        
        loop {
            interval.tick().await;
            
            // Add jitter to prevent thundering herd
            let jitter = self.calculate_jitter();
            sleep(jitter).await;
            
            match self.poll_and_schedule().await {
                Ok(scheduled) => {
                    self.metrics.record_poll_success(scheduled);
                }
                Err(e) => {
                    self.metrics.record_poll_error(&e);
                    tracing::error!("Polling error: {}", e);
                }
            }
        }
    }
    
    async fn poll_and_schedule(&mut self) -> Result<usize, Error> {
        // 1. Update cache if needed
        if self.ready_cache.is_stale() {
            self.refresh_cache().await?;
        }
        
        // 2. Find ready specs
        let ready_specs = self.find_ready_specs().await?;
        
        // 3. Batch schedule
        let batch = SchedulingBatch {
            ready_specs,
            scheduling_time: Utc::now(),
            resource_snapshot: self.ready_cache.resource_availability.clone(),
        };
        
        // 4. Submit to scheduler
        let scheduled_count = self.schedule_batch(batch).await?;
        
        Ok(scheduled_count)
    }
}
```

### Ready State Detection
```rust
impl PollingScheduler {
    async fn find_ready_specs(&self) -> Result<Vec<ReadySpec>, Error> {
        // Query pending specs
        let pending_specs = self.state_manager
            .query_specs(SpecQuery {
                status: Some(ExecutionStatus::Pending),
                limit: Some(100),
                ..Default::default()
            })
            .await?;
        
        // Check each spec
        let mut ready = Vec::new();
        for spec in pending_specs {
            let deps_met = self.check_dependencies(&spec).await?;
            let resources_avail = self.check_resources(&spec);
            
            if deps_met && resources_avail {
                ready.push(ReadySpec {
                    priority: self.calculate_priority(&spec),
                    waiting_time: Utc::now() - spec.created_at,
                    dependencies_met: deps_met,
                    resources_available: resources_avail,
                    spec,
                });
            }
        }
        
        // Sort by priority
        ready.sort_by_key(|r| std::cmp::Reverse(r.priority));
        ready
    }
}
```

## Notes

- Consider implementing adaptive polling intervals based on system load
- Cache invalidation strategy is critical for correctness
- Monitor for polling overhead and optimize query patterns
- Support manual trigger for immediate scheduling checks