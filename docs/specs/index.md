# TaskDaemon Specs Index

This directory contains all specification documents for the TaskDaemon implementation. Each spec represents an atomic unit of work that can be completed in a focused session.

## Spec Overview

| ID | Name | Dependencies | Status |
|----|------|--------------|--------|
| 001 | [LLM Client Trait](001-llm-client-trait.md) | None | Draft |
| 002 | [Anthropic Client Implementation](002-anthropic-client.md) | [001] | Draft |
| 003 | [Loop Engine Core](003-loop-engine-core.md) | [001] | Draft |
| 004 | [System Captured Progress](004-system-captured-progress.md) | [003] | Draft |
| 005 | [Tool System](005-tool-system.md) | [003] | Draft |
| 006 | [Domain Types](006-domain-types.md) | None | Draft |
| 007 | [State Manager Actor](007-state-manager-actor.md) | [006] | Draft |
| 008 | [Crash Recovery System](008-crash-recovery.md) | [006, 007] | Draft |
| 009 | [Execution State Tracking](009-execution-tracking.md) | [006, 007] | Draft |
| 010 | [Coordinator Message Routing](010-coordinator-routing.md) | [003] | Draft |
| 011 | [Priority Queue Scheduler](011-priority-scheduler.md) | [006, 010] | Draft |
| 012 | [Main Branch Watcher](012-main-watcher.md) | [010] | Draft |
| 013 | [Rate Limiting System](013-rate-limiting.md) | [011] | Draft |
| 014 | [Loop Manager](014-loop-manager.md) | [003, 011, 007] | Draft |
| 015 | [Dependency Graph Validation](015-dependency-validation.md) | [006, 011] | Draft |
| 016 | [Polling Scheduler](016-polling-scheduler.md) | [011, 014] | Draft |
| 017 | [Configuration System](017-config-system.md) | None | Draft |
| 018 | [Loop Type Definitions](018-loop-type-definitions.md) | [003, 017] | Draft |
| 019 | [Pipeline Configuration](019-pipeline-config.md) | [018, 017] | Draft |
| 020 | [Template Rendering System](020-template-rendering.md) | [018] | Draft |
| 021 | [Hot Reload System](021-hot-reload.md) | [017, 018] | Draft |
| 022 | [Loop Inheritance System](022-loop-inheritance.md) | [018] | Draft |
| 023 | [Analytics and Metrics System](023-analytics-metrics.md) | [009] | Draft |
| 024 | [Worktree Management](024-worktree-management.md) | [012] | Draft |
| 025 | [Terminal User Interface](025-terminal-ui.md) | [023, 009] | Draft |
| 026 | [CLI Commands](026-cli-commands.md) | [025, 014] | Draft |
| 027 | [Daemon Mode](027-daemon-mode.md) | [026] | Draft |
| 028 | [Project Setup and Directory Structure](028-project-setup.md) | None | Draft |
| 029 | [Core Library Implementation](029-library-implementation.md) | [028] | Draft |
| 030 | [CLI Binary Implementation](030-cli-implementation.md) | [028, 029] | Draft |
| 031 | [Build and Installation](031-build-installation.md) | [028, 029, 030] | Draft |
| 032 | [End-to-End Testing and Validation](032-testing-validation.md) | [028, 029, 030, 031] | Draft |

## Implementation Order

The specs are designed to be implemented in phases that build upon each other:

### Phase 1: Core Ralph Loop Engine
- 001: LLM Client Trait
- 002: Anthropic Client Implementation
- 003: Loop Engine Core
- 004: System Captured Progress
- 005: Tool System

### Phase 2: TaskStore Integration
- 006: Domain Types
- 007: State Manager Actor
- 008: Crash Recovery System
- 009: Execution State Tracking

### Phase 3: Coordination Protocol
- 010: Coordinator Message Routing
- 011: Priority Queue Scheduler
- 012: Main Branch Watcher
- 013: Rate Limiting System

### Phase 4: Multi-Loop Orchestration
- 014: Loop Manager
- 015: Dependency Graph Validation
- 016: Polling Scheduler

### Phase 5: Full Pipeline
- 017: Configuration System
- 018: Loop Type Definitions
- 019: Pipeline Configuration
- 020: Template Rendering System

### Phase 6: Advanced Loop Features
- 021: Hot Reload System
- 022: Loop Inheritance System
- 023: Analytics and Metrics System
- 024: Worktree Management

### Phase 7: TUI & Polish
- 025: Terminal User Interface
- 026: CLI Commands
- 027: Daemon Mode

### Howdy Tool Implementation
- 028: Project Setup and Directory Structure
- 029: Core Library Implementation
- 030: CLI Binary Implementation  
- 031: Build and Installation
- 032: End-to-End Testing and Validation

## Spec Metadata Schema

Each spec follows this structure:

```yaml
id: string           # Unique identifier (e.g., "001-llm-client-trait")
status: enum         # Draft | In Progress | Review | Complete
dependencies: array  # List of spec IDs this depends on
phases: array        # Implementation phases within the spec
estimated_hours: int # Rough estimate of implementation time
```

## Dependency Graph

The specs form a directed acyclic graph of dependencies. Key dependency chains:

1. **LLM Chain**: 001 → 002
2. **Loop Engine Chain**: 001 → 003 → 004/005
3. **State Chain**: 006 → 007 → 008/009
4. **Coordination Chain**: 003 → 010 → 011 → 013
5. **Manager Chain**: 003/007/011 → 014 → 016
6. **Config Chain**: 017 → 018 → 019/020/021/022
7. **UI Chain**: 009/023 → 025 → 026 → 027

## Validation

Before starting a spec, ensure:
1. All dependencies are marked as Complete
2. The spec has been reviewed for completeness
3. Acceptance criteria are clear and testable
4. Technical approach is validated

## Updates

This index should be updated when:
- New specs are added
- Spec status changes
- Dependencies are modified
- Implementation order is adjusted