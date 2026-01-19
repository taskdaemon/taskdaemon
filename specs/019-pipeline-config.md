# Spec: Pipeline Configuration

**ID:** 019-pipeline-config  
**Status:** Draft  
**Dependencies:** [018-loop-type-definitions, 017-config-system]

## Summary

Implement pipeline configuration that wires together the Plan → Spec → Phase cascade, defining how loops trigger subsequent loops and how data flows between them. Support flexible pipeline definitions with validation.

## Acceptance Criteria

1. **Pipeline Definition**
   - Define loop cascades
   - Trigger conditions
   - Data flow mappings
   - Success criteria

2. **Trigger Mechanisms**
   - Completion triggers
   - Conditional triggers
   - Manual triggers
   - Event-based triggers

3. **Data Flow**
   - Output to input mapping
   - Transform functions
   - Validation rules
   - Schema enforcement

4. **Pipeline Control**
   - Start/stop/pause
   - Error handling
   - Rollback support
   - Progress tracking

## Implementation Phases

### Phase 1: Pipeline Model
- Define pipeline types
- Create cascade rules
- Basic trigger system
- Data mapping

### Phase 2: Execution Engine
- Pipeline runner
- State management
- Error handling
- Progress tracking

### Phase 3: Advanced Triggers
- Conditional logic
- Event system
- Manual intervention
- Complex cascades

### Phase 4: Monitoring
- Pipeline visualization
- Execution tracking
- Performance metrics
- Debug tools

## Technical Details

### Module Structure
```
src/pipeline/
├── mod.rs
├── definition.rs  # Pipeline definitions
├── engine.rs      # Execution engine
├── triggers.rs    # Trigger mechanisms
├── flow.rs        # Data flow management
├── state.rs       # Pipeline state
└── monitor.rs     # Monitoring
```

### Pipeline Definition
```rust
pub struct PipelineDefinition {
    pub name: String,
    pub description: String,
    pub stages: Vec<PipelineStage>,
    pub global_config: HashMap<String, Value>,
    pub error_handling: ErrorStrategy,
}

pub struct PipelineStage {
    pub name: String,
    pub loop_type: String,
    pub triggers: Vec<TriggerCondition>,
    pub input_mapping: DataMapping,
    pub output_mapping: DataMapping,
    pub parallel_instances: Option<u32>,
    pub timeout: Duration,
}

pub enum TriggerCondition {
    OnCompletion {
        source_stage: String,
        success_only: bool,
    },
    OnEvent {
        event_type: String,
        filter: Option<EventFilter>,
    },
    OnCondition {
        expression: String,
        variables: Vec<String>,
    },
    Manual {
        approval_required: bool,
    },
}

pub struct DataMapping {
    pub mappings: Vec<FieldMapping>,
    pub transforms: Vec<Transform>,
    pub validation: Option<Schema>,
}
```

### Built-in Pipeline
```rust
pub fn default_taskdaemon_pipeline() -> PipelineDefinition {
    PipelineDefinition {
        name: "taskdaemon".to_string(),
        description: "Standard Plan → Spec → Phase pipeline".to_string(),
        stages: vec![
            PipelineStage {
                name: "planning".to_string(),
                loop_type: "plan".to_string(),
                triggers: vec![TriggerCondition::Manual { approval_required: false }],
                input_mapping: DataMapping {
                    mappings: vec![
                        FieldMapping {
                            from: "request.goal",
                            to: "goal",
                            required: true,
                        },
                    ],
                    transforms: vec![],
                    validation: None,
                },
                output_mapping: DataMapping {
                    mappings: vec![
                        FieldMapping {
                            from: "plan",
                            to: "outputs.plan",
                            required: true,
                        },
                    ],
                    transforms: vec![],
                    validation: Some(plan_schema()),
                },
                parallel_instances: None,
                timeout: Duration::from_secs(1800),
            },
            PipelineStage {
                name: "specification".to_string(),
                loop_type: "spec".to_string(),
                triggers: vec![
                    TriggerCondition::OnCompletion {
                        source_stage: "planning".to_string(),
                        success_only: true,
                    },
                ],
                input_mapping: DataMapping {
                    mappings: vec![
                        FieldMapping {
                            from: "planning.outputs.plan",
                            to: "plan",
                            required: true,
                        },
                    ],
                    transforms: vec![],
                    validation: None,
                },
                output_mapping: DataMapping {
                    mappings: vec![
                        FieldMapping {
                            from: "specs",
                            to: "outputs.specs",
                            required: true,
                        },
                    ],
                    transforms: vec![],
                    validation: Some(spec_array_schema()),
                },
                parallel_instances: None,
                timeout: Duration::from_secs(3600),
            },
            PipelineStage {
                name: "implementation".to_string(),
                loop_type: "phase".to_string(),
                triggers: vec![
                    TriggerCondition::OnEvent {
                        event_type: "spec.ready".to_string(),
                        filter: Some(EventFilter {
                            expression: "spec.dependencies_met == true".to_string(),
                        }),
                    },
                ],
                input_mapping: DataMapping {
                    mappings: vec![
                        FieldMapping {
                            from: "event.spec",
                            to: "spec",
                            required: true,
                        },
                    ],
                    transforms: vec![],
                    validation: None,
                },
                output_mapping: DataMapping {
                    mappings: vec![
                        FieldMapping {
                            from: "implementation",
                            to: "outputs.implementation",
                            required: true,
                        },
                    ],
                    transforms: vec![],
                    validation: None,
                },
                parallel_instances: Some(10), // Run up to 10 specs in parallel
                timeout: Duration::from_secs(7200),
            },
        ],
        global_config: hashmap! {
            "workspace" => json!("/var/taskdaemon/workspaces"),
            "git_branch_prefix" => json!("taskdaemon/"),
        },
        error_handling: ErrorStrategy::Retry {
            max_attempts: 3,
            backoff: ExponentialBackoff::default(),
        },
    }
}
```

### Pipeline Engine
```rust
pub struct PipelineEngine {
    definition: PipelineDefinition,
    state: PipelineState,
    loop_manager: Arc<LoopManager>,
    event_bus: Arc<EventBus>,
}

impl PipelineEngine {
    pub async fn execute(&mut self, input: Value) -> Result<PipelineResult, Error> {
        self.state = PipelineState::Running;
        
        for stage in &self.definition.stages {
            // Wait for triggers
            self.wait_for_triggers(stage).await?;
            
            // Prepare input
            let stage_input = self.map_input(stage, &input)?;
            
            // Execute loop
            let loop_result = self.execute_stage(stage, stage_input).await?;
            
            // Map output
            let stage_output = self.map_output(stage, &loop_result)?;
            
            // Update state
            self.state.stage_outputs.insert(stage.name.clone(), stage_output);
        }
        
        Ok(PipelineResult {
            outputs: self.state.stage_outputs.clone(),
            execution_time: self.state.elapsed(),
        })
    }
}
```

## Notes

- Pipeline definitions should be validated before execution
- Support for branching and conditional paths in future versions
- Consider implementing pipeline templates for common patterns
- Provide good debugging tools for pipeline execution