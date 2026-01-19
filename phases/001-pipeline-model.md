# Phase: Pipeline Model

**ID:** 001-pipeline-model
**Spec:** 019-pipeline-config
**Dependencies:** []
**Status:** Ready

## Summary

Implement the core pipeline model types and definitions that will be used to configure and represent pipelines. This includes the pipeline definition structure, stage configuration, trigger conditions, and data mapping types.

## Acceptance Criteria

1. **Core Types Implementation**
   - [ ] Create `PipelineDefinition` struct with all required fields
   - [ ] Implement `PipelineStage` struct for individual stages
   - [ ] Define `TriggerCondition` enum with all trigger types
   - [ ] Create `DataMapping` and related types
   - [ ] Implement `ErrorStrategy` enum for error handling

2. **Validation**
   - [ ] Validate pipeline definitions for consistency
   - [ ] Check for circular dependencies between stages
   - [ ] Ensure required fields are present
   - [ ] Validate loop type references exist

3. **Default Pipeline**
   - [ ] Implement `default_taskdaemon_pipeline()` function
   - [ ] Include proper stage configurations for Plan → Spec → Phase
   - [ ] Set appropriate timeouts and limits
   - [ ] Configure data mappings between stages

4. **Serialization**
   - [ ] All types must derive `Serialize` and `Deserialize`
   - [ ] Support JSON and YAML formats
   - [ ] Handle optional fields gracefully
   - [ ] Preserve unknown fields for forward compatibility

## Implementation Details

### Files to Create

1. **`src/pipeline/mod.rs`**
   ```rust
   pub mod definition;
   pub mod triggers;
   pub mod flow;
   
   pub use definition::*;
   pub use triggers::*;
   pub use flow::*;
   ```

2. **`src/pipeline/definition.rs`**
   - Main pipeline definition types
   - Validation logic
   - Default pipeline function

3. **`src/pipeline/triggers.rs`**
   - `TriggerCondition` enum and related types
   - Trigger validation logic
   - Helper methods for trigger evaluation

4. **`src/pipeline/flow.rs`**
   - `DataMapping` struct
   - `FieldMapping` struct
   - `Transform` enum
   - Validation schema types

### Test Cases

1. **Pipeline Definition Tests** (`tests/pipeline_definition_test.rs`)
   - Test creating valid pipeline definitions
   - Test validation catches invalid configurations
   - Test serialization/deserialization roundtrip
   - Test default pipeline structure

2. **Trigger Tests** (`tests/pipeline_triggers_test.rs`)
   - Test each trigger condition type
   - Test trigger validation
   - Test complex trigger combinations
   - Test invalid trigger configurations

3. **Data Flow Tests** (`tests/pipeline_flow_test.rs`)
   - Test field mapping configurations
   - Test transform specifications
   - Test validation schema references
   - Test required field enforcement

## Validation Script

```bash
# Run tests for pipeline model
cargo test pipeline_model
cargo test pipeline_definition
cargo test pipeline_triggers
cargo test pipeline_flow

# Check that types compile and are properly exported
cargo check

# Ensure no clippy warnings
cargo clippy -- -D warnings
```

## Example Usage

```rust
use taskdaemon::pipeline::{PipelineDefinition, PipelineStage, TriggerCondition, DataMapping};

// Create a simple pipeline
let pipeline = PipelineDefinition {
    name: "my-pipeline".to_string(),
    description: "Example pipeline".to_string(),
    stages: vec![
        PipelineStage {
            name: "stage1".to_string(),
            loop_type: "plan".to_string(),
            triggers: vec![TriggerCondition::Manual { approval_required: false }],
            input_mapping: DataMapping::default(),
            output_mapping: DataMapping::default(),
            parallel_instances: None,
            timeout: Duration::from_secs(1800),
        },
    ],
    global_config: HashMap::new(),
    error_handling: ErrorStrategy::Fail,
};

// Validate the pipeline
pipeline.validate()?;

// Get the default pipeline
let default = default_taskdaemon_pipeline();
```