# Phase: Pipeline Execution Engine

**ID:** 002-pipeline-execution-engine
**Spec:** 019-pipeline-config
**Dependencies:** [001-pipeline-model]
**Status:** Ready

## Summary

Implement the pipeline execution engine that runs pipeline definitions, manages state, handles errors, and tracks progress. This includes the core execution logic, state management, and integration with the loop manager.

## Acceptance Criteria

1. **Pipeline Engine Implementation**
   - [ ] Create `PipelineEngine` struct with required fields
   - [ ] Implement `execute()` method for running pipelines
   - [ ] Handle stage execution in correct order
   - [ ] Support parallel stage execution where configured

2. **State Management**
   - [ ] Implement `PipelineState` struct for tracking execution
   - [ ] Track stage outputs and intermediate results
   - [ ] Persist state for recovery
   - [ ] Handle state transitions correctly

3. **Trigger Evaluation**
   - [ ] Implement `wait_for_triggers()` method
   - [ ] Support all trigger types from Phase 1
   - [ ] Handle timeout for triggers
   - [ ] Emit proper events for trigger state changes

4. **Data Flow**
   - [ ] Implement input mapping from pipeline inputs to stage inputs
   - [ ] Implement output mapping from stage outputs to pipeline state
   - [ ] Apply transforms as configured
   - [ ] Validate data against schemas

5. **Error Handling**
   - [ ] Implement error strategies (fail, retry, continue)
   - [ ] Handle stage failures gracefully
   - [ ] Support rollback when needed
   - [ ] Log errors appropriately

## Implementation Details

### Files to Create

1. **`src/pipeline/engine.rs`**
   - `PipelineEngine` struct and implementation
   - Core execution logic
   - Integration with LoopManager
   - Error handling logic

2. **`src/pipeline/state.rs`**
   - `PipelineState` struct for tracking execution
   - State persistence and recovery
   - State transition validation
   - Progress tracking

3. **`src/pipeline/executor.rs`**
   - Stage execution logic
   - Parallel execution support
   - Timeout handling
   - Result collection

### Files to Modify

1. **`src/pipeline/mod.rs`**
   - Add new module exports
   - Re-export engine types

2. **`src/lib.rs`**
   - Export pipeline module if not already

### Test Cases

1. **Engine Tests** (`tests/pipeline_engine_test.rs`)
   - Test basic pipeline execution
   - Test stage sequencing
   - Test parallel stage execution
   - Test error handling strategies
   - Test timeout behavior

2. **State Tests** (`tests/pipeline_state_test.rs`)
   - Test state initialization
   - Test state transitions
   - Test state persistence/recovery
   - Test concurrent state updates
   - Test state validation

3. **Integration Tests** (`tests/pipeline_integration_test.rs`)
   - Test full pipeline execution with mock loops
   - Test data flow between stages
   - Test trigger evaluation
   - Test error propagation
   - Test resource cleanup

## Validation Script

```bash
# Run specific tests for execution engine
cargo test pipeline_engine
cargo test pipeline_state
cargo test pipeline_executor
cargo test pipeline_integration

# Check that engine integrates properly
cargo check

# Ensure no clippy warnings
cargo clippy -- -D warnings

# Run a simple example if available
cargo run --example simple_pipeline
```

## Example Usage

```rust
use taskdaemon::pipeline::{PipelineEngine, PipelineDefinition, PipelineResult};
use taskdaemon::loop_manager::LoopManager;

// Create engine with a pipeline definition
let definition = default_taskdaemon_pipeline();
let loop_manager = Arc::new(LoopManager::new());
let event_bus = Arc::new(EventBus::new());

let mut engine = PipelineEngine::new(
    definition,
    loop_manager,
    event_bus,
);

// Execute the pipeline
let input = json!({
    "request": {
        "goal": "Build a new feature"
    }
});

let result = engine.execute(input).await?;

// Access outputs
println!("Pipeline outputs: {:?}", result.outputs);
println!("Execution time: {:?}", result.execution_time);
```

## Performance Considerations

- Use async execution for stage runs
- Implement proper connection pooling for parallel stages
- Cache compiled trigger conditions
- Use efficient data structures for state storage
- Minimize serialization overhead in data mapping

## Security Considerations

- Validate all inputs before execution
- Sanitize data in transforms
- Limit resource usage per stage
- Implement proper access controls
- Audit pipeline executions