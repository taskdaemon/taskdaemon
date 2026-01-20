# Spec: Tool System

**ID:** 005-tool-system
**Status:** Draft
**Dependencies:** [003-loop-engine-core]

## Summary

Implement a comprehensive tool system that provides file operations, command execution, and other utilities to AI loops. Tools should be safe, sandboxed, and produce structured results that can be captured in progress.

## Acceptance Criteria

1. **Core Tools**
   - File read/write/edit operations
   - Command execution with output capture
   - Directory listing and navigation
   - Search functionality (grep/find)

2. **Safety & Sandboxing**
   - Working directory restrictions
   - Command whitelist/blacklist
   - Resource limits (CPU, memory, time)
   - Operation auditing

3. **Tool Registry**
   - Dynamic tool registration
   - Tool metadata and documentation
   - Version management
   - Capability discovery

4. **Result Handling**
   - Structured result format
   - Error propagation
   - Output size limits
   - Result caching

## Implementation Phases

### Phase 1: Tool Framework
- Define tool traits and types
- Create tool registry
- Implement tool discovery
- Build execution pipeline

### Phase 2: File Operations
- Read with line numbers
- Write with backup
- Atomic edit operations
- Directory traversal

### Phase 3: Command Execution
- Shell command runner
- Output capture
- Environment control
- Process management

### Phase 4: Advanced Tools
- Search operations (grep)
- Git operations
- Template rendering
- Custom tool support

## Technical Details

### Module Structure
```
src/tools/
├── mod.rs
├── registry.rs    # Tool registry
├── types.rs       # Tool interfaces
├── execution.rs   # Execution engine
├── file.rs        # File operations
├── command.rs     # Command execution
├── search.rs      # Search tools
└── safety.rs      # Sandboxing logic
```

### Core Types
```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> ToolParameters;

    async fn execute(
        &self,
        params: Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError>;
}

pub struct ToolResult {
    pub success: bool,
    pub output: Value,
    pub metadata: ToolMetadata,
}

pub struct ToolContext {
    pub working_dir: PathBuf,
    pub environment: HashMap<String, String>,
    pub limits: ResourceLimits,
    pub audit_log: AuditLog,
}
```

### Built-in Tools
1. **read**: Read file with line numbers
2. **write**: Write content to file
3. **edit**: Replace text in file
4. **list**: List directory contents
5. **grep**: Search in files
6. **run**: Execute shell command
7. **git**: Git operations

### Safety Measures
- Chroot-style working directory enforcement
- Command sanitization and validation
- Resource limit enforcement via cgroups/ulimit
- Comprehensive operation logging

## Notes

- Tools should be stateless and idempotent where possible
- All tool operations should be reversible or have clear side effects
- Consider implementing a "dry run" mode for testing
- Tool documentation should be sufficient for LLM understanding