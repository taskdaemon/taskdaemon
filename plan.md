# TaskDaemon Implementation Plan

**Status:** Under Review
**Review Pass:** 1/5 - Completeness Check
**Created:** 2025-01-20
**Last Updated:** 2025-01-20

## Summary

Implement TaskDaemon, a distributed task execution system that uses AI-powered loops to autonomously complete software development tasks. The system orchestrates multiple loop types (Plan, Spec, Phase, Ralph) in a hierarchical workflow, each handling different levels of abstraction from high-level requirements to code implementation.

## Project Goals

1. Build a daemon that executes AI-driven development loops autonomously
2. Support hierarchical task decomposition: Plan → Spec → Phase → Ralph
3. Enable multi-loop orchestration with dependency management
4. Provide a TUI for monitoring and interaction
5. Implement crash recovery and state persistence
6. Support hot-reload of loop configurations
7. Ensure scalability to handle 50+ concurrent loops
8. Provide comprehensive error handling and recovery mechanisms

## Core Components

### 1. Ralph Loop Engine
- **LlmClient trait**: Abstract interface for LLM providers
  - Support for streaming responses
  - Error handling and retry logic
  - Token usage tracking
- **AnthropicClient**: Concrete implementation with streaming support
  - API key management
  - Rate limiting compliance
  - Model selection (Claude 3.5 Sonnet)
- **LoopEngine**: Executes iterations with fresh context each time
  - Iteration counter tracking
  - Progress state management
  - Validation hook integration
- **SystemCapturedProgress**: Cross-iteration state management
  - JSONL-based append-only log
  - State recovery on restart
  - Progress deduplication
- **Tools**: File operations (read, write, edit), command execution
  - Sandboxed execution environment
  - Working directory isolation
  - Error propagation

### 2. Domain Model & State Management
- **Domain Types**: Plan, Spec, LoopExecution, Phase
  - Serializable with serde
  - Validation rules
  - State transitions
- **StateManager**: Actor-based state persistence with JSONL storage
  - Atomic state updates
  - Concurrent access control
  - Transaction log
- **Recovery**: Scan for incomplete loops on startup
  - Detect crashed executions
  - Resume from last known state
  - Cleanup orphaned resources
- **Execution tracking**: Status management (Draft, Pending, Running, etc.)
  - State machine enforcement
  - Timestamp tracking
  - Error capture

### 3. Coordination & Scheduling
- **Coordinator**: Message routing (Alert, Query, Share, Stop)
  - Type-safe message passing
  - Broadcast capabilities
  - Dead letter handling
- **Scheduler**: Priority queue with rate limiting
  - Configurable rate limits
  - Priority-based execution
  - Backpressure handling
- **MainWatcher**: Git main branch monitoring for rebasing
  - Periodic polling
  - Conflict detection
  - Automatic rebase attempts
- **Dependency management**: Cycle detection via topological sort
  - Graph validation
  - Deadlock prevention
  - Parallel execution optimization

### 4. Loop Orchestration
- **LoopManager**: Spawns and tracks multiple loops as tokio tasks
  - Task lifecycle management
  - Resource cleanup
  - Health monitoring
- **Concurrency control**: Semaphore-based limits (default: 50)
  - Configurable per loop type
  - Resource pool management
  - Queue overflow handling
- **Polling scheduler**: Checks for ready Specs every 10 seconds
  - Configurable intervals
  - Jitter to prevent thundering herd
  - Skip logic for efficiency
- **Progress tracking**: Updates state across iterations
  - Checkpoint creation
  - Rollback capabilities
  - Progress reporting

### 5. Configuration System
- **Config loading**: Parse taskdaemon.yml at runtime
  - Schema validation
  - Environment variable substitution
  - Default value handling
- **Loop type definitions**: Extensible loop configurations
  - Custom loop types
  - Parameter validation
  - Tool availability
- **Pipeline wiring**: Connect Plan → Spec → Phase cascades
  - Dependency resolution
  - Data flow mapping
  - Error propagation paths
- **Template rendering**: Handlebars-based prompt templates
  - Custom helpers
  - Partial support
  - Context injection

### 6. Advanced Features
- **Hot-reload**: Update loop configs without daemon restart
  - File watching
  - Graceful transition
  - Version management
- **Loop inheritance**: Base configurations for reuse
  - Override mechanisms
  - Composition patterns
  - Validation inheritance
- **Analytics**: Metrics and performance tracking
  - Execution time tracking
  - Token usage statistics
  - Success rate monitoring
- **Worktree management**: Isolated git working directories
  - Automatic creation/cleanup
  - Branch tracking
  - Conflict isolation

### 7. User Interface
- **TUI**: ratatui-based terminal interface
  - Real-time updates
  - Keyboard navigation
  - Color coding for states
- **Views**: Chat, Plan, Executions, Records
  - Scrollable history
  - Search functionality
  - Export capabilities
- **CLI commands**: start, stop, tui, status, new-plan
  - Argument parsing
  - Help documentation
  - Exit code conventions
- **Daemon mode**: Background process with PID file
  - Signal handling
  - Clean shutdown
  - Process monitoring

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

### Phase 1: Core Ralph Loop Engine (Week 1)
1. Implement LlmClient trait and AnthropicClient
   - Define trait interface with async methods
   - Implement Anthropic SDK integration
   - Add streaming response handling
   - Create mock implementation for testing
2. Build LoopEngine with iteration management
   - Create iteration lifecycle hooks
   - Implement context management
   - Add progress tracking
   - Build validation framework
3. Create SystemCapturedProgress strategy
   - Design JSONL storage format
   - Implement append-only writes
   - Add state recovery logic
   - Create progress queries
4. Implement basic tools (file operations, command execution)
   - Build tool registry
   - Implement file read/write/edit
   - Add command execution with timeout
   - Create tool validation

**Deliverables**: Core loop execution with file manipulation capabilities
**Success Metrics**: Successfully execute a simple loop that creates a file

### Phase 2: TaskStore Integration (Week 1-2)
1. Define domain types (Plan, Spec, LoopExecution, Phase)
   - Create Rust structs with serde
   - Define state transitions
   - Add validation methods
   - Create builders for testing
2. Build StateManager actor with message passing
   - Design actor message protocol
   - Implement state mutations
   - Add query capabilities
   - Create subscription mechanism
3. Implement crash recovery scanning
   - Scan state directory on startup
   - Identify incomplete executions
   - Create recovery strategies
   - Add cleanup for orphans
4. Add execution state persistence
   - Define storage schema
   - Implement atomic writes
   - Add transaction logging
   - Create rollback capabilities

**Deliverables**: Persistent state management with crash recovery
**Success Metrics**: System recovers and resumes after forced shutdown

### Phase 3: Coordination Protocol (Week 2)
1. Create Coordinator with message routing
   - Define message types
   - Build routing table
   - Implement pub/sub
   - Add dead letter queue
2. Build Scheduler with priority queue
   - Design priority algorithm
   - Implement queue management
   - Add rate limiting
   - Create backpressure handling
3. Implement MainWatcher for git monitoring
   - Create git polling logic
   - Add branch comparison
   - Implement rebase automation
   - Handle merge conflicts
4. Add rate limiting and backpressure
   - Define rate limit policies
   - Implement token bucket
   - Add circuit breakers
   - Create monitoring hooks

**Deliverables**: Multi-loop coordination with git integration
**Success Metrics**: Multiple loops coordinate without conflicts

### Phase 4: Multi-Loop Orchestration (Week 2-3)
1. Build LoopManager for spawning loops
   - Create loop registry
   - Implement spawn/stop logic
   - Add health checking
   - Build restart policies
2. Add dependency graph validation
   - Implement graph algorithms
   - Add cycle detection
   - Create visualization
   - Build ordering logic
3. Implement polling scheduler
   - Create time-based triggers
   - Add condition checking
   - Implement skip logic
   - Build metrics collection
4. Create semaphore-based concurrency control
   - Define resource pools
   - Implement acquisition logic
   - Add timeout handling
   - Create fairness policies

**Deliverables**: Concurrent loop execution with dependency management
**Success Metrics**: 10+ loops run concurrently without resource conflicts

### Phase 5: Full Pipeline (Week 3)
1. Wire Plan → Spec → Phase cascade
   - Define data flow
   - Implement handoffs
   - Add error propagation
   - Create monitoring
2. Implement config loading from taskdaemon.yml
   - Create YAML schema
   - Add validation logic
   - Implement defaults
   - Build migration tools
3. Parse loop type definitions at runtime
   - Create type registry
   - Add dynamic loading
   - Implement validation
   - Build type checking
4. Connect all components
   - Wire dependency injection
   - Create startup sequence
   - Add shutdown handling
   - Build health checks

**Deliverables**: End-to-end pipeline from Plan to implementation
**Success Metrics**: Complete workflow executes from high-level plan

### Phase 6: Advanced Loop Features (Week 3-4)
1. Add hot-reload capability
   - Implement file watching
   - Create reload logic
   - Add version tracking
   - Build rollback mechanism
2. Implement loop type inheritance
   - Design inheritance model
   - Create override logic
   - Add composition
   - Build validation
3. Build analytics and metrics
   - Define metrics schema
   - Implement collectors
   - Add aggregation
   - Create dashboards
4. Add performance monitoring
   - Track execution times
   - Monitor resource usage
   - Add bottleneck detection
   - Create alerts

**Deliverables**: Production-ready features with monitoring
**Success Metrics**: Hot-reload works without disrupting running loops

### Phase 7: TUI & Polish (Week 4)
1. Create ratatui-based TUI
   - Design layout system
   - Implement rendering
   - Add event handling
   - Create themes
2. Implement all views (Chat, Plan, Executions, Records)
   - Build view models
   - Add navigation
   - Implement search
   - Create filters
3. Add CLI commands
   - Define command structure
   - Implement handlers
   - Add documentation
   - Create completions
4. Implement daemon forking
   - Add process management
   - Create PID handling
   - Implement signals
   - Build monitoring

**Deliverables**: Polished user interface and daemon capabilities
**Success Metrics**: TUI provides real-time visibility into all loops

## Technical Requirements

### Dependencies
- **Runtime**: tokio (async runtime, latest stable)
- **LLM**: anthropic-sdk-rust or custom implementation
- **TUI**: ratatui (0.28+), crossterm (0.28+)
- **Storage**: serde (1.0+), serde_json (1.0+)
- **Git**: git2 (0.19+)
- **Templates**: handlebars (6.0+)
- **Logging**: tracing (0.1+), tracing-subscriber (0.3+)
- **Errors**: thiserror (1.0+), eyre (0.6+)
- **CLI**: clap (4.0+)
- **Config**: config (0.14+), serde_yaml (0.9+)

### Performance Requirements
- Support 50+ concurrent loops
- Sub-100ms TUI response time  
- Graceful degradation under load
- Efficient state persistence (<10ms per write)
- Memory usage under 1GB for typical workloads
- CPU usage scales linearly with active loops

### Security Requirements
- Secure API key storage (environment variables)
- Sandboxed tool execution (no shell escapes)
- No arbitrary code execution
- Audit logging for all actions
- File access limited to worktree
- Network access only to approved endpoints

### Reliability Requirements
- 99.9% uptime for daemon process
- Automatic recovery from crashes
- No data loss on unexpected shutdown
- Graceful handling of LLM API failures
- Resource leak prevention
- Deadlock detection and recovery

## Testing Strategy

### Unit Tests
- All public APIs with comprehensive coverage (>80%)
- Domain logic isolation with property-based testing
- Mock external dependencies (LLM, Git)
- Error path coverage for all modules
- Concurrent execution testing
- Performance regression tests

### Integration Tests
- End-to-end loop execution scenarios
- State persistence and recovery flows
- Multi-loop coordination patterns
- Git operations with conflicts
- Config hot-reload sequences
- Resource exhaustion handling

### System Tests
- Full pipeline execution benchmarks
- TUI interaction flow testing
- Daemon lifecycle management
- Performance under load (50+ loops)
- Memory and CPU profiling
- Network failure simulation

### Testing Infrastructure
- Continuous integration with GitHub Actions
- Test fixtures for common scenarios
- Mock LLM server for deterministic tests
- Git repository fixtures
- Performance benchmarking suite
- Chaos testing framework

## Rollout Plan

1. **Development Environment Setup** (Day 1)
   - Install Rust toolchain
   - Configure development tools
   - Set up testing infrastructure
   - Create project structure

2. **Phase 1-2 Implementation** (Week 1)
   - Core engine development
   - State management implementation
   - Basic testing suite
   - Documentation creation

3. **Phase 3-4 Implementation** (Week 2)
   - Coordination system
   - Orchestration framework
   - Integration testing
   - Performance tuning

4. **Phase 5-6 Implementation** (Week 3)
   - Pipeline assembly
   - Advanced features
   - System testing
   - Load testing

5. **Phase 7 Implementation** (Week 4)
   - TUI development
   - CLI polishing
   - User documentation
   - Release preparation

6. **Testing & Documentation** (Week 5)
   - Comprehensive test execution
   - Documentation review
   - Performance validation
   - Security audit

7. **Beta Release** (Week 5-6)
   - Limited user rollout
   - Feedback collection
   - Bug fixing
   - Performance optimization

8. **Production Release** (Week 6)
   - Full deployment
   - Monitoring setup
   - Support documentation
   - Training materials

## Success Criteria

- All 7 implementation phases complete with tests
- `otto ci` passes all checks (cargo check, clippy, fmt, test)
- 80%+ test coverage across all modules
- TUI responsive and intuitive (<100ms response)
- Successful execution of example workflows
- Documentation complete and accurate
- Performance benchmarks meet requirements
- No critical security vulnerabilities
- Crash recovery works reliably
- Hot-reload functions without disruption

## Risk Mitigation

### Technical Risks
- **LLM API failures**: Implement exponential backoff retry logic, circuit breakers, and graceful degradation
- **State corruption**: JSONL append-only design with checksums, backup strategies, and recovery tools
- **Resource exhaustion**: Semaphore limits, memory monitoring, automatic garbage collection, and backpressure
- **Git conflicts**: MainWatcher with smart rebase handling, conflict detection, and manual intervention hooks
- **Loop failures**: Comprehensive error boundaries, restart policies, and failure isolation

### Operational Risks
- **Scalability issues**: Load testing early, horizontal scaling design, resource pooling
- **Security breaches**: Regular security audits, principle of least privilege, encrypted storage
- **Data loss**: Regular backups, transaction logs, point-in-time recovery
- **Performance degradation**: Continuous monitoring, profiling tools, optimization pipeline
- **Dependency failures**: Vendored dependencies, fallback mechanisms, version pinning

### Project Risks  
- **Scope creep**: Clear phase boundaries, feature flags, MVP focus
- **Timeline delays**: Buffer time per phase, parallel development tracks, regular checkpoints
- **Technical debt**: Code review requirements, refactoring sprints, documentation standards
- **Team scaling**: Modular architecture, clear interfaces, onboarding documentation

## Open Questions

1. **LLM Provider Strategy**: Should we support multiple LLM providers initially or focus on Anthropic?
   - Consider: API compatibility, cost differences, feature parity
   - Recommendation: Start with Anthropic, design for extensibility

2. **Concurrency Limits**: What should the default concurrency limit be for production?
   - Consider: Typical workloads, resource constraints, LLM rate limits
   - Recommendation: Start with 50, make configurable

3. **Timeout Policies**: How should we handle loop timeout policies?
   - Consider: Long-running tasks, cleanup requirements, partial progress
   - Recommendation: Configurable timeouts with graceful shutdown

4. **Multi-Daemon Support**: Should the TUI support multiple daemon connections?
   - Consider: Distributed teams, scaling patterns, complexity
   - Recommendation: Single daemon for MVP, design for future extension

5. **Metrics Exposure**: What metrics should we expose for monitoring?
   - Consider: Operational needs, performance tracking, debugging
   - Recommendation: OpenTelemetry standard metrics plus custom loop metrics

6. **Storage Backend**: Should we support alternative storage backends?
   - Consider: Scale requirements, operational complexity, performance
   - Recommendation: JSONL for MVP, interface design for future backends

## Constraints

- Must use Rust for implementation (performance and reliability requirements)
- Must follow provided module structure (architectural consistency)
- Must implement all 7 phases in order (dependency management)
- Must pass `otto ci` validation (code quality standards)
- Must maintain backward compatibility for configs (user experience)
- Must support Linux and macOS (target platforms)
- Must work with Git 2.0+ (version compatibility)
- Must handle Unicode correctly (internationalization)

## Dependencies on External Teams

None - this is a self-contained project. However, we should coordinate with:
- Security team for API key management best practices
- Infrastructure team for deployment recommendations
- Documentation team for user guide standards

## Migration Strategy

Not applicable - this is a greenfield implementation. Future considerations:
- Design for data format versioning
- Plan for configuration schema evolution
- Consider upgrade paths for long-running daemons

## Maintenance Plan

### Regular Maintenance (Weekly)
- Dependency security updates
- Performance metric review
- Error log analysis
- Documentation updates

### Periodic Maintenance (Monthly)
- Dependency version updates
- Performance optimization based on metrics
- User feedback incorporation
- Security audit review

### Long-term Maintenance (Quarterly)
- Feature additions based on user feedback
- Major dependency upgrades
- Architecture evolution planning
- Deprecation planning

### Support Infrastructure
- GitHub issues for bug tracking
- Discord/Slack for community support
- Documentation site with search
- Video tutorials for common workflows

## Appendix: Key Design Decisions

1. **Actor Model for State**: Chosen for concurrency safety and message passing simplicity
2. **JSONL for Storage**: Human-readable, append-only, easy recovery
3. **Tokio Runtime**: Industry standard for async Rust, great ecosystem
4. **Handlebars Templates**: Familiar syntax, good ecosystem, extensible
5. **Git Worktrees**: Isolation between loops, parallel execution, easy cleanup

---

*This plan represents the complete scope of the TaskDaemon implementation. Each phase builds on the previous, creating a robust and extensible system for AI-powered task execution. The plan has been reviewed for completeness in Review Pass 1.*