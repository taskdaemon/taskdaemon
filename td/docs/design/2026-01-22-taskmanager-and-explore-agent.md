# Design Document: TaskManager Rename and Explore Agent

**Author:** Scott A. Idler
**Date:** 2026-01-22
**Status:** Implemented
**Review Passes Completed:** 5/5

## Summary

Rename `LoopManager` to `TaskManager` to better reflect its role as a general orchestrator of autonomous work units. Introduce an `Explore` agent type that provides read-only codebase investigation capabilities, inspired by Claude Code's Explore subagent pattern.

## Problem Statement

### Background

TaskDaemon currently uses `LoopManager` to orchestrate Ralph Wiggum loops - multi-turn LLM conversations that iterate until validation passes. The "loop" terminology reflects the specific Ralph pattern: restart from scratch, validate, repeat.

Claude Code implements an "Explore" subagent pattern:
- **Subagent**: A child LLM conversation spawned by a parent, with its own context window
- **Read-only**: Only has access to investigation tools (read, grep, glob), cannot modify code
- **Fast model**: Uses Haiku for speed and cost efficiency
- **Isolated context**: Search results stay in the subagent's context, only a summary returns to parent
- **Multi-turn**: Can reason across multiple tool calls to find answers

This pattern is valuable because:
1. Exploration is often verbose (50+ files searched) - isolation keeps parent context clean
2. Investigation doesn't need expensive models - Haiku is sufficient and 10x cheaper
3. Prevents accidental modifications during research

### Problem

1. **Naming mismatch**: `LoopManager` implies it only manages "loops", but we want to spawn different types of autonomous work units (Explore agents, Plan agents) that don't follow the Ralph iteration pattern.

2. **No lightweight exploration**: Currently, investigation requires either:
   - Manual tool calls within an existing loop (pollutes context)
   - Spawning a full Ralph loop (heavyweight, wrong pattern)

3. **Tool vs Agent confusion**: The current `Tool` trait is for single-shot functions. An "explore" capability needs multi-turn LLM reasoning, which is fundamentally different.

### Goals

- Rename `LoopManager` → `TaskManager` with associated type changes
- Define a `Task` abstraction that encompasses different execution patterns
- Implement `Explore` as a lightweight, read-only agent type
- Enable any task to spawn child Explore agents via a new `explore` tool

### Non-Goals

- Changing the Ralph Wiggum loop pattern itself
- Modifying the existing `Tool` trait
- Supporting arbitrary custom agent types (future work)

## Proposed Solution

### Overview

Introduce a `Task` enum that represents different types of autonomous work:

```rust
enum TaskType {
    /// Ralph Wiggum loop: restart-until-validation-passes
    RalphLoop,
    /// Read-only codebase exploration
    Explore,
    /// Plan generation (future)
    Plan,
}
```

`TaskManager` (née `LoopManager`) spawns and manages these tasks. Each task type has its own execution pattern, tool access, and model configuration.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         TaskManager                              │
│  (formerly LoopManager)                                          │
│                                                                  │
│  Responsibilities:                                               │
│  - Spawn tasks as tokio tasks                                    │
│  - Track lifecycle via task registry                             │
│  - Resolve dependencies                                          │
│  - Enforce concurrency limits                                    │
│  - Handle graceful shutdown                                      │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │  Task Registry: HashMap<TaskId, JoinHandle<TaskResult>>     ││
│  └─────────────────────────────────────────────────────────────┘│
│                                                                  │
│  Spawns:                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │ RalphLoop    │  │ Explore      │  │ Plan         │           │
│  │ (LoopEngine) │  │ (ExploreTask)│  │ (PlanTask)   │           │
│  │              │  │              │  │              │           │
│  │ - All tools  │  │ - Read-only  │  │ - Read-only  │           │
│  │ - Validation │  │ - Fast model │  │ - Planning   │           │
│  │ - Iteration  │  │ - Isolated   │  │   prompts    │           │
│  └──────────────┘  └──────────────┘  └──────────────┘           │
└─────────────────────────────────────────────────────────────────┘
```

### Naming Changes

| Current | Proposed | Rationale |
|---------|----------|-----------|
| `LoopManager` | `TaskManager` | Manages tasks, not just loops |
| `LoopManagerConfig` | `TaskManagerConfig` | Follows manager rename |
| `LoopTaskResult` | `TaskResult` | Generic task result |
| `LoopExecution` | `LoopRun` | It's the persisted record of a run, not "the act of executing" |
| `LoopExecutionStatus` | `LoopRunStatus` | Follows LoopRun rename |
| `spawn_loop()` | `spawn_task()` | Generic spawning |
| `max_concurrent_loops` | `max_concurrent_tasks` | Config field |
| `running_count()` | `running_count()` | No change needed |

**Preserved names** (these remain "loop" because they ARE the Ralph pattern):
- `LoopEngine` - Executes the Ralph loop pattern
- `LoopConfig` - Configuration for Ralph loops
- `LoopRun` - Persisted state/record of a loop run (renamed from LoopExecution)
- `loop_types/` directory - Ralph loop type definitions

### Explore Agent Design

#### Characteristics

| Aspect | Value | Rationale |
|--------|-------|-----------|
| Model | Haiku (configurable) | Fast, cheap for exploration |
| Tools | Read-only subset | Cannot modify codebase |
| Context | Isolated | Doesn't pollute parent |
| Max iterations | 3-10 by thoroughness | Short-lived investigation |
| Timeout | 120s (configurable) | Prevents hangs |
| Output | Summary string | Condensed findings |

#### Tool Access

```rust
impl ExploreTask {
    fn allowed_tools() -> Vec<&'static str> {
        vec![
            "read",   // Read files
            "glob",   // Find files by pattern
            "grep",   // Search file contents
            "tree",   // Directory structure
            "list",   // List directory
            "bash",   // Read-only commands only (enforced)
            "query",  // Ask other tasks
        ]
    }

    fn denied_tools() -> Vec<&'static str> {
        vec![
            "write",         // No file creation
            "edit",          // No file modification
            "complete_task", // Not a completing task
            "share",         // Doesn't share state
        ]
    }
}
```

#### Bash Restrictions

For `bash` tool within Explore, enforce read-only via command analysis:

1. **Blocklist approach** (simpler, recommended):
   - Block write commands: `rm`, `rmdir`, `mv`, `cp`, `touch`, `mkdir`, `chmod`, `chown`
   - Block redirects: `>`, `>>` (output redirection)
   - Block dangerous git: `git push`, `git reset`, `git checkout`, `git clean`
   - Allow pipes `|` (needed for `git log | head`, etc.)

2. **Implementation**: Add `read_only: bool` flag to `RunCommandTool` that enables filtering

Note: Perfect sandboxing is difficult. The worktree sandbox already limits scope. Bash restrictions add defense-in-depth but aren't foolproof.

#### Spawning from Tools

New `explore` tool that other tasks can call. **Key design consideration**: Tools are created by `ToolExecutor::standard()` which doesn't have access to TaskManager. Two options:

**Option A: Dependency Injection via ToolContext**
Add an optional `explore_spawner` callback to `ToolContext`:

```rust
pub struct ToolContext {
    // ... existing fields ...

    /// Optional callback for spawning explore tasks
    pub explore_spawner: Option<Arc<dyn ExploreSpawner>>,
}

#[async_trait]
pub trait ExploreSpawner: Send + Sync {
    async fn spawn(&self, config: ExploreConfig) -> Result<String>;
}
```

**Option B: Message Channel**
Add an `mpsc::Sender<ExploreRequest>` to ToolContext that sends to TaskManager:

```rust
pub struct ToolContext {
    // ... existing fields ...

    /// Channel for requesting explore tasks
    pub explore_tx: Option<mpsc::Sender<ExploreRequest>>,
}
```

**Recommended: Option A** - cleaner API, easier testing with mock spawners.

```rust
pub struct ExploreTool;

impl Tool for ExploreTool {
    fn name(&self) -> &'static str { "explore" }

    fn description(&self) -> &'static str {
        "Spawn a read-only exploration agent to investigate the codebase. \
         Returns summarized findings. Use for understanding code structure, \
         finding implementations, or researching patterns."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to investigate"
                },
                "thoroughness": {
                    "type": "string",
                    "enum": ["quick", "medium", "thorough"],
                    "default": "medium",
                    "description": "How deep to search"
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let spawner = match &ctx.explore_spawner {
            Some(s) => s,
            None => return ToolResult::error("Explore not available in this context"),
        };

        let question = input["question"].as_str().unwrap_or("");
        let thoroughness = input["thoroughness"].as_str().unwrap_or("medium");

        let result = spawner
            .spawn(ExploreConfig {
                question: question.to_string(),
                thoroughness: thoroughness.parse().unwrap_or_default(),
                parent_id: Some(ctx.exec_id.clone()),
                worktree: ctx.worktree.clone(),
            })
            .await;

        match result {
            Ok(findings) => ToolResult::success(findings),
            Err(e) => ToolResult::error(format!("Exploration failed: {}", e)),
        }
    }
}
```

### ExploreTask Implementation

**Key difference from LoopEngine**: ExploreTask is a simplified, single-purpose agent. It does NOT:
- Restart from scratch each iteration (no Ralph pattern)
- Run validation commands
- Persist to StateManager
- Merge to git branches

It simply runs a multi-turn conversation until it has an answer.

```rust
/// Lightweight exploration agent - NOT a Ralph loop
pub struct ExploreTask {
    id: String,
    config: ExploreConfig,
    llm: Arc<dyn LlmClient>,
    tools: ToolExecutor,  // Read-only subset
    worktree: PathBuf,
}

impl ExploreTask {
    /// Run exploration and return summary string
    pub async fn run(&mut self) -> Result<String> {
        let mut messages = vec![self.build_system_prompt()];
        let mut iterations = 0;

        loop {
            iterations += 1;
            if iterations > self.config.max_iterations {
                // Force summary if we hit iteration limit
                return Ok(self.summarize_findings(&messages));
            }

            // Call LLM (uses Haiku by default)
            let response = self.llm.complete(&messages, &self.tool_definitions()).await?;
            messages.push(response.to_assistant_message());

            // Check for natural completion (LLM finished without tool calls)
            if response.stop_reason == StopReason::EndTurn {
                return Ok(self.extract_summary(&response));
            }

            // Execute any tool calls
            if let Some(tool_calls) = response.tool_calls {
                let results = self.execute_tools(&tool_calls).await;
                messages.push(self.format_tool_results(&results));
            }
        }
    }

    fn build_system_prompt(&self) -> Message {
        Message::system(format!(
            "You are exploring a codebase to answer a specific question.\n\
             Your findings will be summarized and returned to the requesting task.\n\n\
             Question: {}\n\n\
             Thoroughness: {}\n\n\
             You have read-only access. Use glob, grep, read, and tree to investigate.\n\
             When you have enough information, provide a concise summary of your findings.\n\
             End your final message with a clear SUMMARY section.",
            self.config.question,
            self.config.thoroughness
        ))
    }

    /// Extract summary from final response (look for SUMMARY section or use last text)
    fn extract_summary(&self, response: &LlmResponse) -> String {
        // Implementation: parse response.content for "SUMMARY:" section
        // or fall back to the full text if not found
        todo!()
    }

    /// Force a summary when iteration limit reached
    fn summarize_findings(&self, messages: &[Message]) -> String {
        // Implementation: could make one more LLM call asking for summary,
        // or extract key findings from tool results
        todo!()
    }
}
```

### Data Model

#### ExploreConfig

```rust
pub struct ExploreConfig {
    /// The question to investigate
    pub question: String,

    /// How thorough to be
    pub thoroughness: Thoroughness,

    /// Parent task ID (for context)
    pub parent_id: Option<String>,

    /// Worktree to explore (inherits from parent or uses main)
    pub worktree: PathBuf,

    /// Maximum iterations before forced summary
    pub max_iterations: u32,  // Default: 6

    /// Model to use
    pub model: Option<String>,  // Default: claude-3-haiku

    /// Timeout in seconds (default: 120)
    pub timeout_secs: u32,
}

pub enum Thoroughness {
    Quick,    // max_iterations: 3, surface-level
    Medium,   // max_iterations: 6, reasonable depth (default)
    Thorough, // max_iterations: 10, comprehensive
}

impl Default for Thoroughness {
    fn default() -> Self { Self::Medium }
}

impl Default for ExploreConfig {
    fn default() -> Self {
        Self {
            question: String::new(),
            thoroughness: Thoroughness::default(),
            parent_id: None,
            worktree: PathBuf::from("."),
            max_iterations: 6,
            model: None,  // Uses Haiku
            timeout_secs: 120,
        }
    }
}
```

#### TaskResult (renamed from LoopTaskResult)

```rust
pub enum TaskResult {
    /// Task completed successfully
    Complete {
        task_id: String,
        output: Option<String>,  // For Explore: the summary
        iterations: u32,
    },
    /// Task failed
    Failed {
        task_id: String,
        reason: String,
    },
    /// Task was stopped
    Stopped {
        task_id: String,
    },
}
```

### Implementation Plan

#### Phase 1: Naming Renames

**1a. LoopExecution → LoopRun**

Straightforward find-and-replace. "Run" better represents the persisted record of a loop run.

Files to modify:
- `td/src/domain/execution.rs` → rename struct, update file to `run.rs`
- `td/src/domain/mod.rs` → update exports
- `td/src/state/manager.rs` → update all references
- `td/src/loop/manager.rs` → update all references
- `td/src/loop/cascade.rs` → update references
- `td/docs/domain-types.md` → update documentation

Also rename:
- `LoopExecutionStatus` → `LoopRunStatus`
- `list_executions()` → `list_runs()`
- `get_execution()` → `get_run()`
- `create_execution()` → `create_run()`
- `update_execution()` → `update_run()`

**1b. LoopManager → TaskManager**

1. Rename struct and methods
2. Update all call sites
3. Update documentation

Files to modify:
- `td/src/loop/manager.rs` → rename struct `LoopManager` to `TaskManager`
- `td/src/loop/mod.rs` → re-export as `TaskManager` (keep file in `loop/` for now)
- `td/src/daemon.rs` → update usage
- `td/docs/loop-manager.md` → update doc or create `task-manager.md`

Also rename:
- `LoopManagerConfig` → `TaskManagerConfig`
- `LoopTaskResult` → `TaskResult`
- `spawn_loop()` → `spawn_task()`
- `max_concurrent_loops` → `max_concurrent_tasks`

Note: Keep manager in `td/src/loop/` initially to minimize churn. Can restructure to `td/src/task/` later if we add more task types.

#### Phase 2: Implement Tool Profiles

1. Add `ToolProfile` enum to `ToolExecutor`
2. Implement `definitions_for_profile()` method
3. Add bash command filtering for read-only profile

Files to modify:
- `td/src/tools/executor.rs`
- `td/src/tools/builtin/run_command.rs` (add read-only filtering)
- `td/src/tools/context.rs` (add `explore_spawner` field)

#### Phase 3: Implement ExploreTask

1. Create `td/src/loop/explore.rs` (keep with other execution types for now)
2. Implement exploration logic (simpler than LoopEngine - no validation, no restart)
3. Add `spawn_explore()` method to TaskManager
4. Implement `ExploreSpawner` trait in TaskManager
5. Create `ExploreTool` for spawning from other tasks

New files:
- `td/src/loop/explore.rs` - ExploreTask struct and run logic
- `td/src/tools/builtin/explore.rs` - ExploreTool that calls spawner

Modified files:
- `td/src/loop/mod.rs` - export ExploreTask
- `td/src/loop/manager.rs` - add spawn_explore(), impl ExploreSpawner

#### Phase 4: Integration and Testing

1. Add explore tool to standard tool set
2. Write integration tests
3. Test from existing Ralph loops
4. Verify context isolation

## Alternatives Considered

### Alternative 1: Explore as Enhanced Tool (No LLM)

**Description:** Implement `explore` as a smart search tool that combines glob/grep without LLM reasoning.

**Pros:**
- Simpler implementation
- No additional LLM costs
- Faster execution

**Cons:**
- No reasoning about results
- Can't adapt search based on findings
- Limited to predefined search patterns

**Why not chosen:** The value of Claude Code's Explore comes from the LLM reasoning about what to search next. A static tool misses this key benefit.

### Alternative 2: Keep LoopManager Name

**Description:** Keep `LoopManager` and just add explore as a special "loop type" with different behavior.

**Pros:**
- No renaming effort
- Fewer changes to existing code

**Cons:**
- Confusing: "loop" implies iteration pattern that Explore doesn't follow
- Documentation becomes misleading
- Future agent types would further strain the abstraction

**Why not chosen:** The name `TaskManager` better represents the actual responsibility and allows for cleaner future extensions.

### Alternative 3: Separate ExploreManager

**Description:** Create a completely separate `ExploreManager` alongside `LoopManager`.

**Pros:**
- Clean separation of concerns
- No changes to existing LoopManager

**Cons:**
- Duplicated orchestration logic
- Two managers to coordinate
- Harder to share resources (semaphores, scheduler)

**Why not chosen:** The orchestration patterns are the same - spawn task, track lifecycle, handle completion. One manager should handle all task types.

## Technical Considerations

### Dependencies

**Internal:**
- `ToolExecutor` - needs profile filtering
- `LlmClient` - unchanged, just used with Haiku model
- `StateManager` - may need to track explore tasks (optional)

**External:**
- Anthropic API (Haiku model)

### Performance

| Aspect | Consideration |
|--------|---------------|
| Latency | Haiku is faster than Sonnet (~1.5s vs ~3s per call) |
| Cost | Haiku is ~10x cheaper than Sonnet |
| Concurrency | Explore tasks share the same semaphore as loops |
| Memory | Minimal - short-lived, isolated context |

### Security

- **Read-only enforcement**: Explore cannot modify files
- **Bash filtering**: Only whitelisted commands allowed
- **Worktree isolation**: Operates in existing worktree or main
- **No secrets exposure**: Same restrictions as regular tools

### Testing Strategy

1. **Unit tests**: ExploreTask execution, tool filtering, bash restrictions
2. **Integration tests**: Spawn from Ralph loop, verify isolation
3. **End-to-end**: TUI/CLI trigger explore, verify results

### Rollout Plan

1. Implement behind feature flag
2. Internal testing with existing loops
3. Enable by default
4. Document in user guide

## Edge Cases and Considerations

### Blocking Behavior
The `explore` tool blocks the parent task while waiting. Mitigations:
- **Timeout**: Add `timeout_secs: u32` to ExploreConfig (default: 120s)
- **Cancellation**: If parent is stopped, explore should be cancelled too

### Nested Explores
Should an explore be able to spawn another explore?
- **Recommendation**: No. Set `explore_spawner = None` in ExploreTask's ToolContext
- Prevents infinite recursion and simplifies reasoning

### Rate Limiting
Explore tasks should go through the Scheduler to avoid API overload:
- Register with scheduler before making LLM calls
- Share rate limits with parent loops
- Haiku calls are cheaper but still count against TPM limits

### TUI Visibility
How does the TUI know explores are running?
- **Option 1**: Track in task registry with a different task type flag
- **Option 2**: Don't track (ephemeral, short-lived)
- **Recommendation**: Option 1 - visibility helps debugging

### Token Limits
Haiku has ~200K context but exploration can accumulate verbose output:
- Truncate large tool results (e.g., grep with 100+ matches)
- Already have `max_tokens` in ToolContext for this
- If context fills, force early summary

### Model Switching
ExploreTask needs to use Haiku while parent might use Sonnet:
- LlmClient trait already supports model parameter
- Pass `model: "claude-3-haiku-20240307"` in complete() call
- Or create separate LlmClient instance configured for Haiku

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Rename breaks external integrations | Low | Medium | Type aliases during transition |
| Explore tasks consume too many resources | Medium | Low | Separate concurrency limit for explores |
| Read-only bash bypass | Low | High | Strict blocklist, worktree sandbox as backup |
| Context isolation leaks | Low | Medium | Fresh LLM conversation per explore |
| Explore hangs indefinitely | Low | Medium | Timeout with forced summary |
| Nested explore infinite loop | Low | High | Disable explore tool within explore |
| Token limit exceeded | Medium | Low | Truncate results, force early summary |

## Open Questions

- [ ] Should Explore tasks be persisted in StateManager or ephemeral only?
- [ ] Should there be a separate concurrency limit for Explore vs Ralph tasks?
- [ ] How should Explore handle very large codebases (file count limits)?
- [ ] What's the right default timeout for explores? (120s? 300s?)

## Success Criteria

The implementation is complete when:

1. **Renames complete**: `LoopExecution` → `LoopRun`, `LoopManager` → `TaskManager`, all tests pass
2. **Tool profiles work**: `ToolExecutor` can filter tools by profile, read-only bash enforced
3. **ExploreTask functional**: Can spawn explore, receive summary, respects timeout
4. **Integration works**: Ralph loop can call `explore` tool, receives findings
5. **Isolation verified**: Explore context doesn't leak to parent, nested explores blocked
6. **TUI visibility**: Running explores visible in task list (if option 1 chosen)

## References

- [LoopManager Spec](../loop-manager.md) - Current implementation
- [Tool System](../tools.md) - Tool trait and executor
- [TaskDaemon Design](../taskdaemon-design.md) - Overall architecture
- Claude Code's Explore agent pattern - Inspiration for subagent design
