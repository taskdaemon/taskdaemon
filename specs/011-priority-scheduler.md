# Spec: Priority Queue Scheduler

**ID:** 011-priority-scheduler  
**Status:** Draft  
**Dependencies:** [006-domain-types, 010-coordinator-routing]

## Summary

Build a priority queue-based Scheduler that manages the execution order of loops based on priority, dependencies, and resource constraints. The scheduler ensures fair resource allocation while respecting priorities and preventing starvation.

## Acceptance Criteria

1. **Priority Management**
   - Multi-level priority queue
   - Dynamic priority adjustment
   - Starvation prevention
   - Fair scheduling algorithm

2. **Dependency Handling**
   - Dependency graph validation
   - Topological sorting
   - Deadlock detection
   - Dynamic dependency updates

3. **Resource Management**
   - Concurrency limits
   - Resource allocation
   - Load balancing
   - Backpressure handling

4. **Scheduling Policies**
   - FIFO within priority levels
   - Preemption support
   - Time-slice allocation
   - Custom scheduling strategies

## Implementation Phases

### Phase 1: Queue Infrastructure
- Priority queue implementation
- Task representation
- Basic enqueue/dequeue
- Priority comparison

### Phase 2: Dependency Management
- Dependency graph structure
- Cycle detection
- Topological ordering
- Ready state computation

### Phase 3: Resource Control
- Semaphore implementation
- Resource tracking
- Admission control
- Load monitoring

### Phase 4: Advanced Scheduling
- Anti-starvation mechanisms
- Priority boosting
- Custom policies
- Performance tuning

## Technical Details

### Module Structure
```
src/scheduling/
├── mod.rs
├── scheduler.rs   # Main scheduler
├── queue.rs       # Priority queue
├── dependencies.rs # Dependency graph
├── resources.rs   # Resource management
└── policies.rs    # Scheduling policies
```

### Core Types
```rust
pub struct Scheduler {
    queue: PriorityQueue<ScheduledTask>,
    dependencies: DependencyGraph,
    resources: ResourceManager,
    policy: Box<dyn SchedulingPolicy>,
}

pub struct ScheduledTask {
    pub id: Uuid,
    pub loop_type: LoopType,
    pub priority: Priority,
    pub dependencies: HashSet<Uuid>,
    pub resources_required: ResourceRequirements,
    pub enqueued_at: DateTime<Utc>,
    pub deadline: Option<DateTime<Utc>>,
}

pub struct Priority {
    pub level: u8,  // 0 = highest
    pub boost: u8,  // Anti-starvation boost
    pub age: Duration,
}

pub struct DependencyGraph {
    nodes: HashMap<Uuid, TaskNode>,
    edges: HashMap<Uuid, HashSet<Uuid>>,
    ready_set: HashSet<Uuid>,
}
```

### Scheduling Algorithm
```rust
impl Scheduler {
    pub async fn next_ready(&mut self) -> Option<ScheduledTask> {
        // 1. Update ready set based on completed tasks
        self.dependencies.update_ready_set();
        
        // 2. Apply anti-starvation boosting
        self.apply_priority_aging();
        
        // 3. Find highest priority ready task
        while let Some(task) = self.queue.peek() {
            if self.is_ready(&task) && self.has_resources(&task) {
                return self.queue.pop();
            }
            // Move to waiting set if not ready
            self.move_to_waiting(task);
        }
        None
    }
}
```

### Resource Management
- CPU cores allocation
- Memory limits
- Concurrent execution slots
- Rate limiting per loop type

## Notes

- The scheduler should be fair even under high load
- Consider implementing priority inheritance for dependency chains
- Provide visibility into queue state for debugging
- Design for extensibility with custom scheduling policies