# Spec: Loop Engine Core

**ID:** 003-loop-engine-core
**Status:** Draft
**Dependencies:** [001-llm-client-trait]

## Summary

Build the core `LoopEngine` that executes AI loops with iteration management, fresh context creation, and progress tracking. The engine should support any loop type while maintaining clean separation between iterations.

## Acceptance Criteria

1. **Loop Engine**
   - Generic engine that works with any loop type
   - Fresh context for each iteration
   - Progress capture between iterations
   - Graceful termination support
   - Maximum iteration limits

2. **Iteration Management**
   - Clean context separation
   - Progress accumulation
   - State checkpointing
   - Recovery from interruptions

3. **Tool Integration**
   - Pluggable tool system
   - Tool execution with result capture
   - Error handling for tool failures
   - Tool usage metrics

4. **Extensibility**
   - Support for custom loop types
   - Hook points for monitoring
   - Event emission for state changes
   - Custom termination conditions

## Implementation Phases

### Phase 1: Engine Architecture
- Define `LoopEngine` struct and traits
- Create iteration context management
- Implement basic execution flow
- Add progress tracking types

### Phase 2: Execution Pipeline
- Build prompt template rendering
- Implement LLM interaction layer
- Add tool execution framework
- Create result processing pipeline

### Phase 3: State Management
- Iteration state persistence
- Progress accumulation logic
- Checkpoint/recovery system
- Context window management

### Phase 4: Advanced Features
- Custom termination conditions
- Hook system for monitoring
- Metrics collection
- Performance optimizations

## Technical Details

### Module Structure
```
src/loops/engine/
├── mod.rs
├── engine.rs      # Main LoopEngine
├── context.rs     # Iteration context
├── progress.rs    # Progress tracking
├── tools.rs       # Tool management
└── execution.rs   # Execution flow
```

### Core Types
```rust
pub struct LoopEngine<T: LoopType> {
    llm_client: Box<dyn LlmClient>,
    tools: ToolRegistry,
    config: LoopConfig,
    _phantom: PhantomData<T>,
}

pub trait LoopType: Send + Sync {
    type Context: Send + Sync;
    type Progress: Send + Sync;

    fn create_context(&self, prev_progress: Option<Self::Progress>) -> Self::Context;
    fn extract_progress(&self, result: &ExecutionResult) -> Self::Progress;
}
```

### Execution Flow
1. Initialize with previous progress (if any)
2. Create fresh context for iteration
3. Render prompts with context
4. Execute LLM completion
5. Process tool calls
6. Extract progress
7. Check termination conditions
8. Repeat or complete

## Notes

- The engine should be completely agnostic to specific loop types (Plan, Spec, etc.)
- All loop-specific logic should be in the `LoopType` implementations
- Consider memory efficiency when handling large contexts
- Tool execution should be sandboxed and time-limited