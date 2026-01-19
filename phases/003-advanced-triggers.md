# Phase: Advanced Triggers

**ID:** 003-advanced-triggers
**Spec:** 019-pipeline-config
**Dependencies:** [001-pipeline-model, 002-pipeline-execution-engine]
**Status:** Ready

## Summary

Implement advanced trigger mechanisms including conditional logic evaluation, event system integration, manual intervention support, and complex trigger cascades. This phase extends the basic trigger system from Phase 1 with runtime evaluation and sophisticated trigger conditions.

## Acceptance Criteria

1. **Conditional Trigger Evaluation**
   - [ ] Implement expression parser for condition strings
   - [ ] Support variable references in conditions
   - [ ] Evaluate conditions with proper context
   - [ ] Handle type conversions in expressions
   - [ ] Support logical operators (AND, OR, NOT)

2. **Event System Integration**
   - [ ] Subscribe to system events
   - [ ] Filter events based on criteria
   - [ ] Map event data to trigger context
   - [ ] Handle event timeouts
   - [ ] Support custom event types

3. **Manual Intervention**
   - [ ] Create approval request mechanism
   - [ ] Track approval state
   - [ ] Support approval with conditions
   - [ ] Implement timeout for approvals
   - [ ] Provide approval UI hooks

4. **Complex Cascades**
   - [ ] Support multiple trigger conditions per stage
   - [ ] Implement ANY/ALL logic for triggers
   - [ ] Handle trigger dependencies
   - [ ] Support trigger cancellation
   - [ ] Implement trigger priority

5. **Trigger Monitoring**
   - [ ] Track trigger evaluation history
   - [ ] Log trigger decisions
   - [ ] Emit metrics for trigger performance
   - [ ] Provide debug information

## Implementation Details

### Files to Create

1. **`src/pipeline/triggers/evaluator.rs`**
   - Expression evaluation engine
   - Variable resolution
   - Type handling
   - Context management

2. **`src/pipeline/triggers/event_handler.rs`**
   - Event subscription logic
   - Event filtering
   - Event to trigger mapping
   - Event queue management

3. **`src/pipeline/triggers/manual.rs`**
   - Approval request types
   - Approval state machine
   - Approval storage
   - Timeout handling

4. **`src/pipeline/triggers/cascade.rs`**
   - Complex trigger logic
   - Dependency resolution
   - Priority handling
   - Cascade state tracking

### Files to Modify

1. **`src/pipeline/triggers.rs`**
   - Add advanced trigger logic
   - Integrate new components
   - Update trigger evaluation

2. **`src/pipeline/engine.rs`**
   - Update `wait_for_triggers()` to use new evaluator
   - Add event subscription setup
   - Handle manual approval requests

3. **`src/pipeline/mod.rs`**
   - Export new trigger modules

### Test Cases

1. **Expression Evaluation Tests** (`tests/trigger_expression_test.rs`)
   - Test simple expressions
   - Test complex boolean logic
   - Test variable resolution
   - Test type conversions
   - Test invalid expressions

2. **Event Trigger Tests** (`tests/trigger_event_test.rs`)
   - Test event subscription
   - Test event filtering
   - Test event data mapping
   - Test event timeout
   - Test missing events

3. **Manual Trigger Tests** (`tests/trigger_manual_test.rs`)
   - Test approval creation
   - Test approval state transitions
   - Test approval timeout
   - Test conditional approvals
   - Test approval cancellation

4. **Cascade Tests** (`tests/trigger_cascade_test.rs`)
   - Test multiple triggers
   - Test ANY/ALL logic
   - Test trigger dependencies
   - Test priority ordering
   - Test cascade cancellation

## Validation Script

```bash
# Run trigger-specific tests
cargo test trigger_expression
cargo test trigger_event
cargo test trigger_manual
cargo test trigger_cascade

# Test integration with pipeline engine
cargo test pipeline_trigger_integration

# Check performance of expression evaluation
cargo bench trigger_evaluation

# Ensure no clippy warnings
cargo clippy -- -D warnings
```

## Example Usage

```rust
use taskdaemon::pipeline::{TriggerCondition, EventFilter, PipelineStage};

// Conditional trigger
let conditional = TriggerCondition::OnCondition {
    expression: "outputs.tests_passed == true && metrics.coverage > 80".to_string(),
    variables: vec!["outputs.tests_passed".to_string(), "metrics.coverage".to_string()],
};

// Event-based trigger with filter
let event_trigger = TriggerCondition::OnEvent {
    event_type: "build.complete".to_string(),
    filter: Some(EventFilter {
        expression: "event.branch == 'main' && event.status == 'success'".to_string(),
    }),
};

// Manual approval
let manual = TriggerCondition::Manual {
    approval_required: true,
};

// Complex stage with multiple triggers (ANY logic)
let stage = PipelineStage {
    name: "deploy".to_string(),
    loop_type: "deployment".to_string(),
    triggers: vec![conditional, event_trigger, manual],
    // ... other fields
};
```

## Expression Language

The conditional trigger expression language supports:

- **Comparison**: `==`, `!=`, `>`, `<`, `>=`, `<=`
- **Logical**: `&&`, `||`, `!`
- **Arithmetic**: `+`, `-`, `*`, `/`, `%`
- **String**: `contains()`, `starts_with()`, `ends_with()`
- **Arrays**: `in`, `any()`, `all()`
- **Null-safe**: `?.` optional chaining

Examples:
```
stage.output?.status == "success"
metrics.all(m => m.value > threshold)
tags.contains("production") && !flags.skip_deploy
```