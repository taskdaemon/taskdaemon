# Spec: Loop Manager

**ID:** 014-loop-manager  
**Status:** Draft  
**Dependencies:** [003-loop-engine-core, 011-priority-scheduler, 007-state-manager-actor]

## Summary

Build the LoopManager that spawns and tracks multiple loops as tokio tasks, managing their lifecycle, monitoring their health, and coordinating their execution within system resource constraints.

## Acceptance Criteria

1. **Loop Lifecycle Management**
   - Spawn loops as tokio tasks
   - Track running loops
   - Graceful shutdown
   - Restart failed loops

2. **Resource Management**
   - Enforce concurrency limits
   - Memory usage tracking
   - CPU allocation
   - Task prioritization

3. **Health Monitoring**
   - Heartbeat checking
   - Stuck loop detection
   - Performance metrics
   - Error tracking

4. **Coordination**
   - Inter-loop communication
   - State synchronization
   - Event propagation
   - Dependency handling

## Implementation Phases

### Phase 1: Task Management
- Loop spawning system
- Task handle tracking
- Lifecycle methods
- Basic monitoring

### Phase 2: Resource Control
- Semaphore-based limits
- Resource allocation
- Priority handling
- Load balancing

### Phase 3: Health System
- Heartbeat mechanism
- Health checks
- Auto-restart logic
- Metric collection

### Phase 4: Advanced Features
- Loop pools
- Dynamic scaling
- Performance tuning
- Debug capabilities

## Technical Details

### Module Structure
```
src/loops/manager/
├── mod.rs
├── manager.rs     # Main LoopManager
├── spawner.rs     # Loop spawning logic
├── monitor.rs     # Health monitoring
├── resources.rs   # Resource management
└── registry.rs    # Loop registry
```

### Core Types
```rust
pub struct LoopManager {
    loops: Arc<RwLock<HashMap<LoopId, ManagedLoop>>>,
    scheduler: Arc<Scheduler>,
    resources: ResourceManager,
    coordinator: Arc<Coordinator>,
    config: LoopManagerConfig,
}

pub struct ManagedLoop {
    pub id: LoopId,
    pub loop_type: LoopType,
    pub task_handle: JoinHandle<Result<(), LoopError>>,
    pub state: LoopState,
    pub health: HealthStatus,
    pub metrics: LoopMetrics,
    pub created_at: DateTime<Utc>,
}

pub struct ResourceManager {
    concurrency_limit: Arc<Semaphore>,
    memory_tracker: MemoryTracker,
    cpu_allocator: CpuAllocator,
}

pub struct LoopManagerConfig {
    pub max_concurrent_loops: usize,
    pub default_loop_timeout: Duration,
    pub health_check_interval: Duration,
    pub auto_restart_policy: RestartPolicy,
}
```

### Loop Spawning
```rust
impl LoopManager {
    pub async fn spawn_loop(&self, request: LoopRequest) -> Result<LoopId, Error> {
        // Acquire resources
        let permit = self.resources.concurrency_limit.acquire().await?;
        
        // Create loop instance
        let loop_id = LoopId::new();
        let engine = self.create_engine(&request)?;
        
        // Spawn task
        let handle = tokio::spawn(async move {
            let _permit = permit; // Hold permit for lifetime
            engine.run().await
        });
        
        // Register loop
        let managed_loop = ManagedLoop {
            id: loop_id.clone(),
            loop_type: request.loop_type,
            task_handle: handle,
            state: LoopState::Running,
            health: HealthStatus::Healthy,
            metrics: LoopMetrics::default(),
            created_at: Utc::now(),
        };
        
        self.loops.write().await.insert(loop_id.clone(), managed_loop);
        Ok(loop_id)
    }
}
```

### Health Monitoring
```rust
pub struct HealthMonitor {
    check_interval: Duration,
    timeout_threshold: Duration,
    heartbeat_tracker: HashMap<LoopId, Instant>,
}

impl HealthMonitor {
    async fn monitor_loop(&self, loop_id: &LoopId) -> HealthStatus {
        // Check if loop is responsive
        // Monitor resource usage
        // Detect stuck conditions
        // Return health status
    }
}
```

## Notes

- Use structured concurrency patterns to ensure clean shutdown
- Implement graceful degradation when approaching resource limits
- Consider implementing loop pooling for frequently used loop types
- Provide comprehensive debugging tools for loop state inspection