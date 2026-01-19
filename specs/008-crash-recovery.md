# Spec: Crash Recovery System

**ID:** 008-crash-recovery  
**Status:** Draft  
**Dependencies:** [006-domain-types, 007-state-manager-actor]

## Summary

Implement a robust crash recovery system that scans for incomplete loops on startup and provides mechanisms to resume or clean up interrupted executions. This ensures the system can recover gracefully from unexpected shutdowns.

## Acceptance Criteria

1. **Recovery Scanning**
   - Detect all incomplete executions on startup
   - Identify recoverable vs non-recoverable states
   - Generate recovery reports
   - Automatic recovery for safe cases

2. **State Reconstruction**
   - Rebuild in-memory state from JSONL logs
   - Verify state consistency
   - Detect and handle corrupted data
   - Progress restoration

3. **Recovery Actions**
   - Resume interrupted loops
   - Mark stale executions as failed
   - Clean up temporary resources
   - Notify about manual interventions needed

4. **Monitoring**
   - Recovery metrics
   - Failure pattern detection
   - Health status reporting
   - Recovery audit logs

## Implementation Phases

### Phase 1: State Scanning
- Implement startup scanner
- Parse JSONL history
- Identify incomplete work
- Build recovery plan

### Phase 2: State Validation
- Consistency checking
- Corruption detection
- Relationship validation
- Progress verification

### Phase 3: Recovery Actions
- Automatic resume logic
- Failure marking
- Resource cleanup
- Manual intervention API

### Phase 4: Monitoring & Reporting
- Recovery dashboard
- Metrics collection
- Alert generation
- Audit trail

## Technical Details

### Module Structure
```
src/store/recovery/
├── mod.rs
├── scanner.rs     # Startup scanner
├── validator.rs   # State validation
├── actions.rs     # Recovery actions
├── report.rs      # Recovery reporting
└── monitor.rs     # Health monitoring
```

### Recovery Process
```rust
pub struct RecoveryScanner {
    state_manager: Arc<StateManager>,
    storage_path: PathBuf,
}

pub struct RecoveryReport {
    pub incomplete_plans: Vec<RecoveryItem<Plan>>,
    pub incomplete_specs: Vec<RecoveryItem<Spec>>,
    pub orphaned_executions: Vec<LoopExecution>,
    pub corrupted_records: Vec<CorruptedRecord>,
    pub recommended_actions: Vec<RecoveryAction>,
}

pub enum RecoveryAction {
    ResumeLoop { execution_id: Uuid },
    MarkFailed { execution_id: Uuid, reason: String },
    CleanupResources { paths: Vec<PathBuf> },
    ManualIntervention { description: String },
}
```

### Recovery Strategies
1. **Safe Resume**: For recently interrupted loops with valid state
2. **Mark Failed**: For loops interrupted beyond recovery timeout
3. **State Rebuild**: Reconstruct from last known good snapshot
4. **Manual Review**: For complex cases requiring human decision

### Health Monitoring
```rust
pub struct HealthStatus {
    pub last_recovery: DateTime<Utc>,
    pub recovery_success_rate: f64,
    pub common_failure_patterns: Vec<FailurePattern>,
    pub system_health: SystemHealth,
}
```

## Notes

- Recovery should be idempotent - running multiple times should be safe
- Consider implementing a "recovery mode" that limits system functionality
- Keep detailed logs of all recovery actions for post-mortem analysis
- Design for gradual recovery to avoid overwhelming the system on startup