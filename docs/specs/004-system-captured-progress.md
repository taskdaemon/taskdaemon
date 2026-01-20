# Spec: System Captured Progress

**ID:** 004-system-captured-progress
**Status:** Draft
**Dependencies:** [003-loop-engine-core]

## Summary

Implement the `SystemCapturedProgress` strategy for managing cross-iteration state. This system automatically captures relevant information from each iteration and makes it available to subsequent iterations without requiring explicit progress tracking in prompts.

## Acceptance Criteria

1. **Progress Capture**
   - Automatic extraction of key information from LLM responses
   - Tool execution results capture
   - File modification tracking
   - Error and warning collection

2. **State Representation**
   - Structured progress format
   - Efficient serialization
   - Version compatibility
   - Size limits to prevent context explosion

3. **Context Integration**
   - Seamless injection into next iteration
   - Relevant progress filtering
   - Summary generation for large histories
   - Progress visualization utilities

4. **Persistence**
   - Save progress between iterations
   - Recovery from interruptions
   - Progress replay capabilities
   - Audit trail maintenance

## Implementation Phases

### Phase 1: Progress Types
- Define progress data structures
- Create capture strategies
- Implement basic extraction
- Add serialization support

### Phase 2: Capture System
- LLM response parsing
- Tool result integration
- File change detection
- Metadata collection

### Phase 3: Context Building
- Progress summarization
- Relevance filtering
- Context size management
- Template integration

### Phase 4: Advanced Features
- Custom capture rules
- Progress analytics
- Visualization support
- Debug utilities

## Technical Details

### Module Structure
```
src/loops/progress/
├── mod.rs
├── capture.rs     # Progress capture logic
├── types.rs       # Progress data types
├── context.rs     # Context building
├── storage.rs     # Persistence layer
└── summary.rs     # Summarization logic
```

### Core Types
```rust
pub struct SystemProgress {
    pub iterations: Vec<IterationProgress>,
    pub cumulative_state: CumulativeState,
    pub metadata: ProgressMetadata,
}

pub struct IterationProgress {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub llm_summary: Option<String>,
    pub tools_used: Vec<ToolExecution>,
    pub files_modified: Vec<FileChange>,
    pub errors: Vec<ProgressError>,
}

pub trait ProgressCapture: Send + Sync {
    fn capture_llm_response(&mut self, response: &str);
    fn capture_tool_result(&mut self, tool: &str, result: &ToolResult);
    fn capture_error(&mut self, error: &dyn Error);
    fn finalize(self) -> IterationProgress;
}
```

### Capture Strategies
1. **Automatic Extraction**: Parse structured data from responses
2. **Tool Tracking**: Record all tool invocations and results
3. **Change Detection**: Monitor file system modifications
4. **Error Collection**: Aggregate all errors with context

## Notes

- Progress should be self-contained and not require external references
- Consider implementing progress "compression" for long-running loops
- The system should handle progress format evolution gracefully
- Provide clear boundaries on what constitutes "progress" vs temporary state