# Spec: Configuration System

**ID:** 017-config-system  
**Status:** Draft  
**Dependencies:** None

## Summary

Build a comprehensive configuration system that loads settings from taskdaemon.yml, supports environment variable overrides, validates configurations, and enables runtime updates for certain settings.

## Acceptance Criteria

1. **Configuration Loading**
   - Parse YAML configuration files
   - Environment variable overrides
   - Default value handling
   - Multiple config file support

2. **Validation**
   - Schema validation
   - Type checking
   - Range validation
   - Dependency checking

3. **Runtime Updates**
   - Hot-reload for safe settings
   - Change notifications
   - Rollback on errors
   - Audit logging

4. **Type Safety**
   - Strongly typed configurations
   - Compile-time guarantees
   - Migration support
   - Backward compatibility

## Implementation Phases

### Phase 1: Core Loading
- YAML parser integration
- Basic type definitions
- Environment variable support
- Default configurations

### Phase 2: Validation System
- Schema definitions
- Validation rules
- Error reporting
- Configuration testing

### Phase 3: Runtime Reload
- File watching
- Safe reload logic
- Change propagation
- Rollback mechanism

### Phase 4: Advanced Features
- Configuration inheritance
- Template support
- Secret management
- Performance optimization

## Technical Details

### Module Structure
```
src/config/
├── mod.rs
├── types.rs       # Configuration types
├── loader.rs      # Loading logic
├── validator.rs   # Validation rules
├── runtime.rs     # Runtime updates
├── schema.rs      # Schema definitions
└── defaults.rs    # Default values
```

### Configuration Types
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDaemonConfig {
    pub daemon: DaemonConfig,
    pub loops: HashMap<String, LoopTypeConfig>,
    pub storage: StorageConfig,
    pub llm: LlmConfig,
    pub limits: LimitConfig,
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub port: u16,
    pub host: String,
    pub pid_file: PathBuf,
    pub work_dir: PathBuf,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTypeConfig {
    pub template_path: PathBuf,
    pub max_iterations: u32,
    pub timeout: Duration,
    pub tools: Vec<String>,
    pub inherits: Option<String>,
    pub variables: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LlmConfig {
    Anthropic {
        api_key: SecretString,
        model: String,
        max_tokens: u32,
    },
    OpenAI {
        api_key: SecretString,
        model: String,
        max_tokens: u32,
    },
}
```

### Loading System
```rust
pub struct ConfigLoader {
    search_paths: Vec<PathBuf>,
    env_prefix: String,
    validators: Vec<Box<dyn ConfigValidator>>,
}

impl ConfigLoader {
    pub async fn load(&self) -> Result<TaskDaemonConfig, ConfigError> {
        // 1. Find config file
        let config_path = self.find_config_file()?;
        
        // 2. Load YAML
        let mut config: TaskDaemonConfig = self.load_yaml(&config_path)?;
        
        // 3. Apply environment overrides
        self.apply_env_overrides(&mut config)?;
        
        // 4. Apply defaults
        self.apply_defaults(&mut config)?;
        
        // 5. Resolve inheritance
        self.resolve_inheritance(&mut config)?;
        
        // 6. Validate
        self.validate(&config)?;
        
        Ok(config)
    }
    
    fn apply_env_overrides(&self, config: &mut TaskDaemonConfig) -> Result<(), ConfigError> {
        // TASKDAEMON_DAEMON_PORT=8080
        // TASKDAEMON_LLM_API_KEY=sk-...
        // etc.
    }
}
```

### Hot Reload
```rust
pub struct ConfigRuntime {
    current: Arc<RwLock<TaskDaemonConfig>>,
    loader: ConfigLoader,
    watchers: Vec<Box<dyn ConfigWatcher>>,
}

impl ConfigRuntime {
    pub async fn watch_for_changes(&mut self) {
        let (tx, mut rx) = mpsc::channel(10);
        
        // Set up file watcher
        let mut watcher = notify::recommended_watcher(move |res| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        })?;
        
        loop {
            match rx.recv().await {
                Some(event) => {
                    if self.should_reload(&event) {
                        match self.reload().await {
                            Ok(new_config) => {
                                self.notify_watchers(&new_config).await;
                            }
                            Err(e) => {
                                tracing::error!("Config reload failed: {}", e);
                            }
                        }
                    }
                }
                None => break,
            }
        }
    }
}
```

### Example Configuration
```yaml
daemon:
  host: 0.0.0.0
  port: 3000
  work_dir: /var/taskdaemon
  log_level: info

loops:
  plan:
    template_path: templates/plan.hbs
    max_iterations: 50
    timeout: 30m
    tools: [read, write, edit]
    
  spec:
    inherits: plan
    template_path: templates/spec.hbs
    max_iterations: 20
    
storage:
  type: jsonl
  path: /var/taskdaemon/store
  compaction_interval: 1h
  
llm:
  type: anthropic
  api_key: ${ANTHROPIC_API_KEY}
  model: claude-3-opus-20240229
  max_tokens: 4000

limits:
  max_concurrent_loops: 50
  max_memory_per_loop: 512MB
  global_rate_limit: 1000/min
```

## Notes

- Sensitive values like API keys should never be logged
- Configuration changes should be atomic - all or nothing
- Support both YAML and TOML for flexibility
- Provide clear error messages for configuration issues