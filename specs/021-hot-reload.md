# Spec: Hot Reload System

**ID:** 021-hot-reload
**Status:** Draft
**Dependencies:** [017-config-system, 018-loop-type-definitions]

## Summary

Implement hot-reload capability that allows updating loop configurations without daemon restart. The system should safely apply changes, validate new configurations, and rollback on errors while maintaining system stability.

## Acceptance Criteria

1. **Change Detection**
   - File system monitoring
   - Configuration diffing
   - Change categorization
   - Reload triggers

2. **Safe Reload**
   - Validate before apply
   - Atomic updates
   - Rollback on failure
   - Zero downtime

3. **Scope Management**
   - Identify reloadable settings
   - Block unsafe changes
   - Partial reload support
   - Dependency tracking

4. **State Preservation**
   - Running loop protection
   - Queue preservation
   - Connection maintenance
   - Progress continuity

## Implementation Phases

### Phase 1: Change Detection
- File watcher setup
- Change detection logic
- Diff generation
- Event system

### Phase 2: Validation System
- Pre-reload validation
- Compatibility checking
- Impact analysis
- Safety verification

### Phase 3: Reload Engine
- Atomic updates
- Rollback mechanism
- State preservation
- Error handling

### Phase 4: Integration
- UI notifications
- Audit logging
- Metrics tracking
- Debug tooling

## Technical Details

### Module Structure
```
src/reload/
├── mod.rs
├── watcher.rs     # File monitoring
├── diff.rs        # Change detection
├── validator.rs   # Validation logic
├── reloader.rs    # Reload engine
├── rollback.rs    # Rollback system
└── state.rs       # State preservation
```

### Core Types
```rust
pub struct HotReloadManager {
    config_path: PathBuf,
    current_config: Arc<RwLock<TaskDaemonConfig>>,
    watcher: FileWatcher,
    validator: ConfigValidator,
    subscribers: Vec<Box<dyn ReloadSubscriber>>,
    reload_history: Vec<ReloadEvent>,
}

pub struct ReloadEvent {
    pub timestamp: DateTime<Utc>,
    pub changes: ConfigDiff,
    pub result: ReloadResult,
    pub rollback_point: Option<ConfigSnapshot>,
}

pub enum ReloadResult {
    Success {
        applied_changes: Vec<Change>,
        warnings: Vec<String>,
    },
    Failed {
        error: ReloadError,
        rollback_performed: bool,
    },
    Rejected {
        reason: String,
        unsafe_changes: Vec<UnsafeChange>,
    },
}

pub struct ConfigDiff {
    pub added: HashMap<String, Value>,
    pub modified: HashMap<String, (Value, Value)>, // (old, new)
    pub removed: HashMap<String, Value>,
    pub reload_scope: ReloadScope,
}

pub enum ReloadScope {
    Full,               // Requires full restart
    LoopTypes,          // Can reload loop definitions
    Limits,             // Can adjust limits
    Templates,          // Can reload templates
    Monitoring,         // Can update monitoring
}
```

### Change Detection
```rust
impl HotReloadManager {
    pub async fn watch(&mut self) -> Result<(), Error> {
        let (tx, mut rx) = mpsc::channel(10);

        self.watcher.watch(&self.config_path, move |event| {
            if event.kind.is_modify() {
                let _ = tx.try_send(event);
            }
        })?;

        while let Some(event) = rx.recv().await {
            if let Err(e) = self.handle_change(event).await {
                tracing::error!("Hot reload failed: {}", e);
                self.notify_subscribers(ReloadEvent {
                    timestamp: Utc::now(),
                    changes: self.last_diff.clone(),
                    result: ReloadResult::Failed {
                        error: e,
                        rollback_performed: true,
                    },
                    rollback_point: Some(self.create_snapshot()),
                }).await;
            }
        }

        Ok(())
    }

    async fn handle_change(&mut self, event: Event) -> Result<(), ReloadError> {
        // Load new config
        let new_config = self.load_config().await?;

        // Generate diff
        let diff = self.generate_diff(&self.current_config.read().await, &new_config)?;

        // Validate changes
        self.validate_reload(&diff, &new_config)?;

        // Apply changes
        self.apply_reload(diff, new_config).await
    }
}
```

### Validation System
```rust
impl HotReloadManager {
    fn validate_reload(&self, diff: &ConfigDiff, new_config: &TaskDaemonConfig) -> Result<(), ReloadError> {
        // Check reload scope
        let scope = self.determine_reload_scope(diff);

        // Reject if full restart required
        if matches!(scope, ReloadScope::Full) {
            return Err(ReloadError::UnsafeChange {
                reason: "Changes require full restart".to_string(),
                changes: self.list_unsafe_changes(diff),
            });
        }

        // Validate new configuration
        self.validator.validate(new_config)?;

        // Check for breaking changes
        if let Some(breaking) = self.find_breaking_changes(diff) {
            return Err(ReloadError::BreakingChange(breaking));
        }

        // Check running loops compatibility
        if !self.check_running_loops_compatible(diff).await? {
            return Err(ReloadError::IncompatibleWithRunning);
        }

        Ok(())
    }

    fn determine_reload_scope(&self, diff: &ConfigDiff) -> ReloadScope {
        // Analyze which subsystems are affected
        let mut scope = ReloadScope::Templates;

        for key in diff.modified.keys().chain(diff.added.keys()).chain(diff.removed.keys()) {
            match key.split('.').next() {
                Some("daemon") => return ReloadScope::Full,
                Some("storage") => return ReloadScope::Full,
                Some("loops") => scope = scope.max(ReloadScope::LoopTypes),
                Some("limits") => scope = scope.max(ReloadScope::Limits),
                _ => {}
            }
        }

        scope
    }
}
```

### Reload Application
```rust
impl HotReloadManager {
    async fn apply_reload(&mut self, diff: ConfigDiff, new_config: TaskDaemonConfig) -> Result<(), ReloadError> {
        // Create rollback point
        let snapshot = self.create_snapshot();

        // Begin atomic update
        let update_guard = self.begin_atomic_update().await?;

        match self.perform_reload(&diff, new_config).await {
            Ok(()) => {
                // Commit the update
                update_guard.commit().await?;

                // Notify subscribers
                self.notify_subscribers(ReloadEvent {
                    timestamp: Utc::now(),
                    changes: diff,
                    result: ReloadResult::Success {
                        applied_changes: self.list_applied_changes(&diff),
                        warnings: self.collect_warnings(&diff),
                    },
                    rollback_point: Some(snapshot),
                }).await;

                Ok(())
            }
            Err(e) => {
                // Rollback
                update_guard.rollback().await?;
                self.restore_snapshot(snapshot).await?;
                Err(e)
            }
        }
    }

    async fn perform_reload(&mut self, diff: &ConfigDiff, new_config: TaskDaemonConfig) -> Result<(), ReloadError> {
        match diff.reload_scope {
            ReloadScope::Templates => {
                // Reload templates only
                self.reload_templates(&new_config).await?;
            }
            ReloadScope::Limits => {
                // Update rate limits and resource limits
                self.reload_limits(&new_config).await?;
                self.reload_templates(&new_config).await?;
            }
            ReloadScope::LoopTypes => {
                // Reload loop type definitions
                self.reload_loop_types(&new_config).await?;
                self.reload_limits(&new_config).await?;
                self.reload_templates(&new_config).await?;
            }
            _ => return Err(ReloadError::UnsupportedScope),
        }

        // Update current config
        *self.current_config.write().await = new_config;
        Ok(())
    }
}
```

### Subscriber Interface
```rust
#[async_trait]
pub trait ReloadSubscriber: Send + Sync {
    async fn on_reload(&self, event: &ReloadEvent);
}

// Example subscribers
pub struct LoggingSubscriber;

#[async_trait]
impl ReloadSubscriber for LoggingSubscriber {
    async fn on_reload(&self, event: &ReloadEvent) {
        match &event.result {
            ReloadResult::Success { applied_changes, warnings } => {
                tracing::info!(
                    "Configuration reloaded successfully. {} changes applied, {} warnings",
                    applied_changes.len(),
                    warnings.len()
                );
            }
            ReloadResult::Failed { error, rollback_performed } => {
                tracing::error!(
                    "Configuration reload failed: {}. Rollback: {}",
                    error,
                    rollback_performed
                );
            }
            ReloadResult::Rejected { reason, unsafe_changes } => {
                tracing::warn!(
                    "Configuration reload rejected: {}. {} unsafe changes detected",
                    reason,
                    unsafe_changes.len()
                );
            }
        }
    }
}
```

## Notes

- Hot reload should never interrupt running loops
- Provide clear feedback about what can and cannot be reloaded
- Consider implementing a dry-run mode to preview changes
- Maintain audit trail of all configuration changes