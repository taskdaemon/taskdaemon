# TaskDaemon Implementation Guide

## Your Mission

Implement TaskDaemon across ALL 7 phases. You are running in a Ralph Wiggum loop - each iteration starts fresh with no memory of previous work. You MUST check what's already done and continue from there.

## CRITICAL: How This Loop Works

1. You have NO memory of previous iterations
2. State is preserved ONLY in files and git commits
3. You MUST check `git log` and `src/` to see what's already implemented
4. You MUST continue from where the previous iteration left off
5. You MUST NOT exit until ALL 7 phases are complete

## Step 1: Assess Current State (DO THIS FIRST)

Before doing ANYTHING else, run these commands:

```bash
# What phases have been committed?
git log --oneline | head -20

# What modules exist?
ls -la src/

# What's the current test count?
cargo test 2>&1 | tail -5
```

Then determine: **Which phase should I work on next?**

## Step 2: Phase Checklist

Check each phase against what exists in `src/`:

| Phase | Required Modules | How to Check |
|-------|------------------|--------------|
| **1** | `src/llm/`, `src/loop/engine.rs`, `src/progress.rs`, `src/tools/` | `ls src/llm src/loop src/tools` |
| **2** | `src/domain/`, `src/state/` | `ls src/domain src/state` |
| **3** | `src/coordinator/`, `src/scheduler/`, `src/watcher/` | `ls src/coordinator src/scheduler src/watcher` |
| **4** | `src/loop/manager.rs` with LoopManager | `grep -l "LoopManager" src/loop/*.rs` |
| **5** | Full pipeline wiring in config + main | `grep -l "Pipeline" src/*.rs` |
| **6** | Hot-reload, loop inheritance | `grep -l "hot.reload\|inheritance" src/**/*.rs` |
| **7** | `src/tui/`, CLI commands in main.rs | `ls src/tui && grep "Command::" src/main.rs` |

## Step 3: Implement the Next Incomplete Phase

For the phase you identified:

1. **Read the design docs** for that phase (see table below)
2. **Implement the code** following the design
3. **Write tests** for all public functions
4. **Run `otto ci`** and fix any issues
5. **Commit** with message format: `feat(scope): description - Phase N of 7`

## Step 4: Loop Back to Step 1

After committing a phase:
- **DO NOT EXIT**
- Go back to Step 1
- Check if there are more phases to implement
- Continue until ALL 7 phases are done

## Step 5: Exit Condition

You may ONLY create the completion marker when:
1. ALL 7 phases are committed (check `git log`)
2. ALL modules from the checklist exist
3. `otto ci` passes

**When ALL 7 phases are complete, create the sentinel file:**
```bash
echo "All 7 phases complete - $(date)" > .taskdaemon-complete
```

**DO NOT create this file until ALL phases are done.** The loop uses this file to know when to stop.

---

## Phase Details

| Phase | Focus | Key Docs |
|-------|-------|----------|
| **1** | Core Ralph Loop Engine | [loop-engine.md](./docs/loop-engine.md), [llm-client.md](./docs/llm-client.md), [progress-strategy.md](./docs/progress-strategy.md) |
| **2** | TaskStore Integration | [domain-types.md](./docs/domain-types.md), [implementation-details.md](./docs/implementation-details.md) |
| **3** | Coordination Protocol | [coordinator-design.md](./docs/coordinator-design.md) |
| **4** | Multi-Loop Orchestration | [loop-manager.md](./docs/loop-manager.md), [scheduler.md](./docs/scheduler.md) |
| **5** | Full Pipeline | [config-schema.md](./docs/config-schema.md), [tools.md](./docs/tools.md) |
| **6** | Advanced Loop Features | [execution-model-design.md](./docs/execution-model-design.md) |
| **7** | TUI & Polish | [tui-design.md](./docs/tui-design.md) |

### Phase Deliverables

**Phase 1: Core Ralph Loop Engine**
- `LlmClient` trait + `AnthropicClient` with streaming
- `LoopEngine` that executes iterations with fresh context
- `SystemCapturedProgress` for cross-iteration state
- Basic tools: read_file, write_file, edit_file, run_command

**Phase 2: TaskStore Integration**
- Domain types: Plan, Spec, LoopExecution, Phase
- StateManager actor with message passing
- Crash recovery: scan_for_recovery, resume incomplete loops

**Phase 3: Coordination Protocol**
- Coordinator with Alert/Query/Share/Stop
- Scheduler with priority queue + rate limiting
- MainWatcher for git main branch monitoring

**Phase 4: Multi-Loop Orchestration**
- LoopManager that spawns/tracks multiple loops as tokio tasks
- Dependency graph validation (cycle detection via topological sort)
- Polling scheduler (check for ready Specs every 10s)
- Semaphore for concurrency limits (default: 50)

**Phase 5: Full Pipeline**
- Wire Plan → Spec → Phase cascade
- Config loading from taskdaemon.yml
- Loop type definitions parsed at runtime

**Phase 6: Advanced Loop Features**
- Hot-reload loop configs without daemon restart
- Loop type inheritance
- Loop analytics and metrics

**Phase 7: TUI & Polish**
- ratatui TUI showing all loops
- CLI commands: start, stop, tui, status, new-plan
- Daemon forking (background process with PID file)

---

## Project Structure

```
src/
├── lib.rs              # Public API exports
├── main.rs             # CLI + daemon entry point
├── config.rs           # Configuration types and loading
├── cli.rs              # CLI command definitions
├── llm/
│   ├── mod.rs
│   ├── client.rs       # LlmClient trait
│   ├── anthropic.rs    # AnthropicClient implementation
│   └── types.rs        # Request/Response types
├── loop/
│   ├── mod.rs
│   ├── engine.rs       # LoopEngine iteration logic
│   ├── manager.rs      # LoopManager orchestration (Phase 4)
│   ├── config.rs       # Loop configuration
│   └── validation.rs   # Validation runner
├── progress/
│   ├── mod.rs
│   └── strategy.rs     # ProgressStrategy trait + SystemCapturedProgress
├── tools/
│   ├── mod.rs
│   ├── context.rs      # ToolContext (worktree-scoped)
│   ├── executor.rs     # ToolExecutor
│   └── builtin/        # read_file, write_file, etc.
├── domain/
│   ├── mod.rs
│   └── types.rs        # Plan, Spec, LoopExecution, Phase
├── state/
│   ├── mod.rs
│   ├── manager.rs      # StateManager actor
│   └── recovery.rs     # Crash recovery
├── coordinator/
│   ├── mod.rs
│   └── core.rs         # Alert/Query/Share routing
├── scheduler/
│   ├── mod.rs
│   └── core.rs         # Priority queue + rate limiting
├── watcher/
│   ├── mod.rs
│   └── main_watcher.rs # Git main branch monitoring
├── worktree/
│   ├── mod.rs
│   └── manager.rs      # Git worktree management
└── tui/                # Phase 7
    ├── mod.rs
    └── views/
```

---

## Rust Conventions

1. **Use dependency injection** - Accept traits, not concrete types
2. **Return data, not side effects** - Functions return `Result<T>`
3. **Async all the way** - Use `tokio` runtime, `async fn` throughout
4. **Structured errors** - Use `thiserror` for error types, `eyre` for propagation
5. **Always use `cargo add`** - Never manually write dependency versions

### Validation

Run before each commit:

```bash
otto ci
```

This runs: cargo check, cargo clippy, cargo fmt --check, cargo test

---

## Commit Message Format

```
feat(scope): description

Phase N of 7: <phase name>
```

---

## What NOT to Do

- Don't skip checking current state first
- Don't exit before all 7 phases are complete
- Don't manually write dependency versions - use `cargo add`
- Don't skip `otto ci` validation
- Don't commit without tests

---

## References

- [taskdaemon.yml](./taskdaemon.yml) - Example config with loop definitions
- [docs/taskdaemon-design.md](./docs/taskdaemon-design.md) - Master architecture
- [docs/](./docs/) - All design documentation
