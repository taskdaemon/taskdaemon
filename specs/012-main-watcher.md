# Spec: Main Branch Watcher

**ID:** 012-main-watcher
**Status:** Draft
**Dependencies:** [010-coordinator-routing]

## Summary

Implement the MainWatcher component that monitors the git main branch for changes and coordinates rebasing of work branches. This ensures that loops work on up-to-date code and handles merge conflicts gracefully.

## Acceptance Criteria

1. **Git Monitoring**
   - Poll or watch for main branch updates
   - Detect relevant changes
   - Filter by paths/patterns
   - Batch change notifications

2. **Rebase Coordination**
   - Notify affected loops of updates
   - Coordinate rebase timing
   - Handle merge conflicts
   - Preserve work in progress

3. **Conflict Resolution**
   - Detect merge conflicts early
   - Provide conflict information to loops
   - Support automatic resolution strategies
   - Manual intervention escalation

4. **Safety Measures**
   - Backup before rebase
   - Atomic rebase operations
   - Rollback capabilities
   - Work preservation

## Implementation Phases

### Phase 1: Git Integration
- Git repository abstraction
- Branch monitoring setup
- Change detection logic
- Event generation

### Phase 2: Change Analysis
- Diff parsing
- Impact analysis
- Path filtering
- Change categorization

### Phase 3: Rebase Orchestration
- Rebase scheduling
- Loop coordination
- Conflict detection
- State preservation

### Phase 4: Conflict Handling
- Conflict analysis
- Resolution strategies
- Manual escalation
- Recovery procedures

## Technical Details

### Module Structure
```
src/git/watcher/
├── mod.rs
├── monitor.rs     # Branch monitoring
├── analyzer.rs    # Change analysis
├── rebase.rs      # Rebase operations
├── conflicts.rs   # Conflict handling
└── backup.rs      # Backup management
```

### Core Types
```rust
pub struct MainWatcher {
    repo: Repository,
    main_branch: String,
    last_seen_commit: Option<Oid>,
    watch_interval: Duration,
    coordinator: Arc<Coordinator>,
}

pub struct BranchUpdate {
    pub from_commit: Oid,
    pub to_commit: Oid,
    pub changed_files: Vec<PathChange>,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub impact: ImpactAnalysis,
}

pub struct RebaseRequest {
    pub loop_id: LoopId,
    pub work_branch: String,
    pub target_commit: Oid,
    pub strategy: RebaseStrategy,
    pub conflict_handler: ConflictHandler,
}

pub enum ConflictHandler {
    Automatic(ConflictStrategy),
    Manual { notify: LoopId },
    Abort,
}
```

### Monitoring Strategy
```rust
impl MainWatcher {
    async fn watch_loop(&mut self) {
        loop {
            if let Some(update) = self.check_for_updates()? {
                let affected_loops = self.analyze_impact(&update);

                for loop_id in affected_loops {
                    self.coordinator.send(CoordinationMessage::Alert {
                        severity: AlertSeverity::Info,
                        message: "Main branch updated".to_string(),
                        context: json!({ "update": update }),
                    }).await?;
                }
            }

            sleep(self.watch_interval).await;
        }
    }
}
```

### Rebase Safety
1. Create backup branch before rebase
2. Validate repository state
3. Perform rebase with conflict detection
4. Rollback on failure
5. Notify loop of results

## Notes

- Consider implementing webhooks for instant notifications instead of polling
- Rebase operations should be queued to prevent concurrent modifications
- Maintain detailed logs of all rebase operations for troubleshooting
- Support for different merge strategies (rebase, merge, squash)