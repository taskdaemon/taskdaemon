# TaskDaemon Implementation Plan

**Status:** Draft
**Review Pass:** 1/5 - Completeness
**Created:** 2025-01-20

## Summary

Implement TaskDaemon, a distributed task execution system that uses AI-powered loops to autonomously complete software development tasks. The system orchestrates multiple loop types (Plan, Spec, Phase, Ralph) in a hierarchical workflow, each handling different levels of abstraction from high-level requirements to code implementation.

## Project Goals

1. Build a daemon that executes AI-driven development loops autonomously
2. Support hierarchical task decomposition: Plan → Spec → Phase → Ralph
3. Enable multi-loop orchestration with dependency management
4. Provide a TUI for monitoring and interaction
5. Implement crash recovery and state persistence
6. Support hot-reload of loop configurations

## Core Components

### 1. Ralph Loop Engine
- **LlmClient trait**: Abstract interface for LLM providers
- **AnthropicClient**: Concrete implementation with streaming support
- **LoopEngine**: Executes iterations with fresh context each time
- **SystemCapturedProgress**: Cross-iteration state management
- **Tools**: File operations (read, write, edit), command execution

### 2. Domain Model & State Management
- **Domain Types**: Plan, Spec, LoopExecution, Phase
- **StateManager**: Actor-based state persistence with JSONL storage
- **Recovery**: Scan for incomplete loops on startup
- **Execution tracking**: Status management (Draft, Pending, Running, etc.)

### 3. Coordination & Scheduling
- **Coordinator**: Message routing (Alert, Query, Share, Stop)
- **Scheduler**: Priority queue with rate limiting
- **MainWatcher**: Git main branch monitoring for rebasing
- **Dependency management**: Cycle detection via topological sort

### 4. Loop Orchestration
- **LoopManager**: Spawns and tracks multiple loops as tokio tasks
- **Concurrency control**: Semaphore-based limits (default: 50)
- **Polling scheduler**: Checks for ready Specs every 10 seconds
- **Progress tracking**: Updates state across iterations

### 5. Configuration System
- **Config loading**: Parse taskdaemon.yml at runtime
- **Loop type definitions**: Extensible loop configurations
- **Pipeline wiring**: Connect Plan → Spec → Phase cascades
- **Template rendering**: Handlebars-based prompt templates

### 6. Advanced Features
- **Hot-reload**: Update loop configs without daemon restart
- **Loop inheritance**: Base configurations for reuse
- **Analytics**: Metrics and performance tracking
- **Worktree management**: Isolated git working directories

### 7. User Interface
- **TUI**: ratatui-based terminal interface
- **Views**: Chat, Plan, Executions, Records
- **CLI commands**: start, stop, tui, status, new-plan
- **Daemon mode**: Background process with PID file

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                            CLI/TUI                               │
├─────────────────────────────────────────────────────────────────┤
│                         Loop Manager                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │ Plan Loop   │  │ Spec Loop   │  │ Phase Loop  │            │
│  └─────────────┘  └─────────────┘  └─────────────┘            │
├─────────────────────────────────────────────────────────────────┤
│                    Core Infrastructure                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │ Loop Engine │  │Coordinator  │  │ Scheduler   │            │
│  └─────────────┘  └─────────────┘  └─────────────┘            │
├─────────────────────────────────────────────────────────────────┤
│                     State & Storage                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │StateManager │  │ Worktree    │  │ TaskStore   │            │
│  └─────────────┘  └─────────────┘  └─────────────┘            │
└─────────────────────────────────────────────────────────────────┘
```

## Implementation Phases

### Phase 1: Core Ralph Loop Engine
1. Implement LlmClient trait and AnthropicClient
2. Build LoopEngine with iteration management
3. Create SystemCapturedProgress strategy
4. Implement basic tools (file operations, command execution)

### Phase 2: TaskStore Integration
1. Define domain types (Plan, Spec, LoopExecution, Phase)
2. Build StateManager actor with message passing
3. Implement crash recovery scanning
4. Add execution state persistence

### Phase 3: Coordination Protocol
1. Create Coordinator with message routing
2. Build Scheduler with priority queue
3. Implement MainWatcher for git monitoring
4. Add rate limiting and backpressure

### Phase 4: Multi-Loop Orchestration
1. Build LoopManager for spawning loops
2. Add dependency graph validation
3. Implement polling scheduler
4. Create semaphore-based concurrency control

### Phase 5: Full Pipeline
1. Wire Plan → Spec → Phase cascade
2. Implement config loading from taskdaemon.yml
3. Parse loop type definitions at runtime
4. Connect all components

### Phase 6: Advanced Loop Features
1. Add hot-reload capability
2. Implement loop type inheritance
3. Build analytics and metrics
4. Add performance monitoring

### Phase 7: TUI & Polish
1. Create ratatui-based TUI
2. Implement all views (Chat, Plan, Executions, Records)
3. Add CLI commands
4. Implement daemon forking

## Technical Requirements

### Dependencies
- **Runtime**: tokio (async runtime)
- **LLM**: anthropic-sdk-rust or custom implementation
- **TUI**: ratatui, crossterm
- **Storage**: serde, serde_json
- **Git**: git2
- **Templates**: handlebars
- **Logging**: tracing, tracing-subscriber
- **Errors**: thiserror, eyre

### Performance Requirements
- Support 50+ concurrent loops
- Sub-100ms TUI response time
- Graceful degradation under load
- Efficient state persistence

### Security Requirements
- Secure API key storage
- Sandboxed tool execution
- No arbitrary code execution
- Audit logging for actions

## Testing Strategy

### Unit Tests
- All public APIs with comprehensive coverage
- Domain logic isolation
- Mock external dependencies
- Property-based testing for complex algorithms

### Integration Tests
- End-to-end loop execution
- State persistence and recovery
- Multi-loop coordination
- Git operations

### System Tests
- Full pipeline execution
- TUI interaction flows
- Daemon lifecycle management
- Performance benchmarks

## Rollout Plan

1. **Development Environment**: Set up with all dependencies
2. **Phase 1-2 Implementation**: Core engine and state (Week 1)
3. **Phase 3-4 Implementation**: Coordination and orchestration (Week 2)
4. **Phase 5-6 Implementation**: Pipeline and advanced features (Week 3)
5. **Phase 7 Implementation**: TUI and polish (Week 4)
6. **Testing & Documentation**: Comprehensive testing (Week 5)
7. **Beta Release**: Limited rollout for feedback
8. **Production Release**: Full deployment

## Success Criteria

- All 7 implementation phases complete
- `otto ci` passes all checks
- 80%+ test coverage
- TUI responsive and intuitive
- Successful execution of example workflows
- Documentation complete and accurate

## Risk Mitigation

- **LLM API failures**: Implement retry logic and graceful degradation
- **State corruption**: JSONL append-only design with recovery
- **Resource exhaustion**: Semaphore limits and backpressure
- **Git conflicts**: MainWatcher and rebase handling
- **Loop failures**: Comprehensive error handling and recovery

## Open Questions

1. Should we support multiple LLM providers initially or focus on Anthropic?
2. What should the default concurrency limit be for production?
3. How should we handle loop timeout policies?
4. Should the TUI support multiple daemon connections?
5. What metrics should we expose for monitoring?

## Constraints

- Must use Rust for implementation
- Must follow provided module structure
- Must implement all 7 phases in order
- Must pass `otto ci` validation
- Must maintain backward compatibility for configs

## Dependencies on External Teams

None - this is a self-contained project.

## Migration Strategy

Not applicable - this is a greenfield implementation.

## Maintenance Plan

- Regular dependency updates
- Performance monitoring and optimization
- Feature additions based on user feedback
- Security patches as needed
- Documentation updates with each release

---

*This plan represents the complete scope of the TaskDaemon implementation. Each phase builds on the previous, creating a robust and extensible system for AI-powered task execution.*