# Output Handling Analysis: Otto vs TaskDaemon

> **Status:** Analysis complete. See [Event Bus Architecture Design](design/2026-01-21-event-bus-architecture.md) for the implementation plan that addresses the issues identified here.

## Key Architectural Differences

| Aspect | Otto (subprocess) | TaskDaemon (async tasks) |
|--------|-------------------|--------------------------|
| **Capture Method** | `Command::spawn()` with piped streams | `Command::output()` waits for completion |
| **Output Timing** | Real-time streaming, line-by-line | Complete capture at command end |
| **File Writing** | TeeWriter writes to files during execution | Database storage after completion |
| **Storage Location** | `~/.otto/<project>/<timestamp>/tasks/<task>/stdout.log` | Embedded in IterationLog database records |
| **TUI Updates** | Broadcast channels + file tailing | Database queries (no live streaming) |
| **Live Updates** | Yes - via broadcast channel subscription | No - data only available after validation completes |

## Otto's Streaming Architecture

Otto uses a **TeeWriter pattern** that writes to multiple destinations simultaneously:

```
Subprocess → piped stdout/stderr
                    ↓
              TeeWriter
              ↓      ↓        ↓
           File   Terminal  Broadcast Channel
         (always)  (if not   (TUI subscribes)
                    TUI)
```

**File structure:**
```
~/.otto/<project-hash>/<timestamp>/
├── tasks/
│   └── <task-name>/
│       ├── stdout.log      # Real-time written during execution
│       ├── stderr.log      # Real-time written during execution
│       └── output.json     # Final results
```

## TaskDaemon's Current Architecture

TaskDaemon captures **complete output at command completion**:

```
Validation Command → .output() → ValidationResult (stdout, stderr)
                                        ↓
                               IterationLog (database)
                                        ↓
                               TUI queries database
```

**Current storage:**
- Full output in `IterationLog` records (database collection)
- Truncated progress in `LoopExecution.progress` (500 chars per iteration)
- No file-based output storage for tasks

---

## Can We Make TD Follow Similar Behavior?

**Yes, but it requires changes to the validation execution model:**

### Required Changes

1. **Switch from `.output()` to `.spawn()` with piped streams**
   - Current: `Command::new("sh").args(["-c", &cmd]).output().await`
   - New: `Command::new("sh").args(["-c", &cmd]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()`

2. **Add structured output directory**
   - Suggested path: `~/.taskdaemon/runs/<execution-id>/iterations/<n>/`
   - Files: `stdout.log`, `stderr.log`, `metadata.json`

3. **Implement TeeWriter or equivalent**
   - Write to file in real-time
   - Optionally broadcast to channel for TUI subscription

4. **Add workspace concept for file paths**
   - Similar to otto's `Workspace` struct that provides deterministic paths

### Implementation Complexity: Medium-High

The core challenge is that `run_validation()` in `td/src/loop/validation.rs` currently uses the blocking `.output()` pattern. Converting to streaming requires:

1. Spawning the process
2. Creating async tasks to read stdout/stderr line-by-line
3. Writing each line to files
4. Optionally broadcasting to channels
5. Waiting for process completion
6. Handling timeouts properly with the streaming approach

---

## Can [o] Output and [l] Logs Tail Those Files?

**Yes, with two possible approaches:**

### Approach 1: File Tailing (simpler but less efficient)

The TUI could use `tokio::fs::File` with `notify` crate or periodic polling to tail the log files:

```rust
// Pseudo-code
let stdout_path = workspace.stdout_path(execution_id, iteration);
let mut file = File::open(&stdout_path).await?;
file.seek(SeekFrom::End(0)).await?; // Start at end

loop {
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).await? > 0 {
        // New content - update TUI
        tx.send(LogUpdate(buf)).await?;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
}
```

### Approach 2: Broadcast Channels (otto's approach - more elegant)

Create a broadcast channel during validation and let TUI subscribe:

```rust
// In validation execution
let (tx, _) = broadcast::channel::<TaskOutput>(10000);

// TUI subscribes
let mut rx = output_broadcaster.subscribe();
while let Ok(output) = rx.recv().await {
    update_logs_view(output);
}
```

**Recommendation:** Broadcast channels are better because:
- No polling overhead
- Immediate updates
- No file system dependency for live view
- Files still written for persistence/history

---

## Key Difference: Subprocess vs Async Tasks

The fundamental difference:

- **Otto's tasks ARE subprocesses** - shell commands running in separate processes with piped I/O
- **TD's async tasks** include validation commands (subprocess) but also LLM interactions (async HTTP)

For **validation commands**, we CAN adopt otto's approach since they're also subprocesses.

For **LLM tool execution** in REPL mode, we already capture tool results - these aren't subprocesses, they're async Rust code. The current approach of storing results in `ReplMessage` is appropriate.

---

## Recommended Implementation Plan

> **Note:** This preliminary plan has been superseded by the detailed [Event Bus Architecture Design](design/2026-01-21-event-bus-architecture.md), which includes specific files to modify, code examples, and edge case handling.

1. **Add workspace/output directory structure**
   - Path: `~/.taskdaemon/runs/<execution-id>/`

2. **Create streaming validation runner**
   - Uses `spawn()` + piped streams
   - Writes to files + broadcasts to channel

3. **Add broadcast channel to validation context**
   - TUI subscribes to receive live updates

4. **Update TUI views to support live streaming**
   - `[l] Logs` and `[o] Output` show live streaming content

5. **Keep database storage for history**
   - Files provide live view; database provides historical queries

---

## Summary

| Question | Answer |
|----------|--------|
| Can TD adopt otto's pattern? | Yes - validation commands are subprocesses too |
| Can we drop files somewhere known? | Yes - `~/.taskdaemon/runs/<exec-id>/iterations/<n>/` |
| Can TUI tail files for live updates? | Yes - via file tailing OR broadcast channels |
| Which approach is better? | Broadcast channels (like otto) + files for persistence |
| Effort required | Medium-high - requires refactoring validation execution |

The main blocker is that `run_validation()` uses `.output()` which blocks until completion. Converting to streaming with `.spawn()` + piped I/O enables both file writing and broadcast channels for live TUI updates.

> **Next Step:** The [Event Bus Architecture Design](design/2026-01-21-event-bus-architecture.md) provides the complete implementation plan, addressing both validation streaming (Phase 4) and LLM streaming (Phase 3).

---

## Important Clarification: Process Overhead Misconception

The choice between `.output()` and `.spawn()` does **not** affect whether a process is created - both spawn a subprocess. The difference is purely in I/O handling:

| Method | Process Created? | How Output is Handled |
|--------|------------------|----------------------|
| `.output().await` | Yes | Waits for completion, returns complete stdout/stderr |
| `.spawn()` + piped streams | Yes | Returns immediately, you read streams incrementally |

The async/await approach in TD is still valuable for:
- LLM API calls (async HTTP - no processes)
- Concurrent operations
- Orchestration layer

But validation commands that run external tools (`otto ci`, `cargo build`) **must** spawn subprocesses - there's no way around this.

---

## The Bigger Picture: What Users Actually Want to See

The analysis above focuses on validation output, but the real goal is visibility into the **entire agentic loop**:

```
[Prompt] → Decomposing task into specs...
[Response] → Created 3 specs: auth, validation, storage
[Prompt] → Breaking spec 'auth' into phases...
[Response] → Phase 1: Add OAuth provider...
[Tool Call] → write_file: src/auth.rs
[Tool Result] → ✓ written
[Prompt] → Running validation...
[Validation] → exit 1: type error on line 42
[Prompt] → Fixing type error...
[Response] → Changed return type to Option<User>
...
```

This requires an **event stream** architecture:

1. **Event types:**
   - `PromptSent { summary, tokens }`
   - `ResponseChunk { text }` (streaming)
   - `ToolCall { name, args }`
   - `ToolResult { name, output }`
   - `ValidationStarted { command }`
   - `ValidationCompleted { exit_code, output }`

2. **Broadcast channel** - TUI subscribes for live updates

3. **File persistence** - Write events to `~/.taskdaemon/runs/<id>/events.jsonl`

This plays to async's strengths - it's all event-driven, non-blocking I/O. The existing `ConversationLog` infrastructure can be extended to:
- Be always-on (not debug-only)
- Feed a broadcast channel for TUI subscription
- Have a structured format for display

---

## Implementation

The full implementation plan for addressing these issues is documented in:

**[Event Bus Architecture Design](design/2026-01-21-event-bus-architecture.md)**

Key phases that address this analysis:
- **Phase 3: LLM Streaming** - Switches from `complete()` to `stream()` with event emission
- **Phase 4: Validation Streaming** - Switches from `.output()` to `.spawn()` with piped streams
