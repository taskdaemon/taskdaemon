# Design Document: Event Bus Architecture for Live Observability

**Author:** Claude (with Scott)
**Date:** 2026-01-21
**Status:** Ready for Review
**Review Passes Completed:** 5/5

## Summary

TaskDaemon currently operates as a black box - tasks go in, results come out, but the journey is invisible. This design introduces an event bus as the architectural spine, enabling real-time visibility into the agentic loop: prompts sent, LLM responses streaming, tool calls executing, and validation results appearing - all visible as they happen.

## Problem Statement

### Background

TaskDaemon (TD) was built to automate software engineering tasks through an agentic loop (RALPH: Reason, Act, Learn, Persist, Halt). The system decomposes problems into specs, specs into phases, and iterates on each phase until validation passes.

The current architecture prioritizes *function* over *observability*. A detailed analysis comparing TD's approach to otto-rs identified two key blocking patterns (see [Output Handling Analysis](../output-handling-analysis.md)):

| Current Pattern | Problem | Otto's Approach |
|-----------------|---------|-----------------|
| `Command::output()` | Waits for validation to complete before any output visible | `Command::spawn()` with piped streams for real-time output |
| `LlmClient::complete()` | Waits for full LLM response before any tokens visible | Streaming with broadcast channels |
| TUI polls every 250ms | Latency gap; not true real-time | Event subscription with immediate delivery |

The analysis concluded that TD could adopt otto's streaming patterns since validation commands are also subprocesses, and the existing `LlmClient::stream()` method is already implemented but unused in the RALPH loop.

### Problem

**Users cannot watch TD work.** The system accepts tasks and eventually completes them, but provides no window into:
- How problems are being decomposed (specs → phases)
- What prompts are being sent to the LLM
- What the LLM is responding with (token by token)
- What tools are being called and why
- Why validation failed and what's being tried next

This makes TD feel like a black box rather than an intelligent assistant you can observe and understand.

### Goals

1. **Real-time visibility**: See every LLM interaction, tool call, and validation result as it happens
2. **Streaming output**: LLM responses stream token-by-token to the TUI
3. **Structured event log**: All events persisted to files for history, debugging, and replay
4. **Minimal latency**: Events appear in TUI within milliseconds, not 250ms polling intervals
5. **Clean architecture**: Event bus as the single source of truth for all activity

### Non-Goals

- Changing the RALPH loop logic or agentic behavior
- Modifying how tasks are decomposed (that's working)
- Adding new tools or capabilities
- Changing the validation command approach
- Real-time collaboration or multi-user features

## Proposed Solution

### Overview

Introduce an **Event Bus** as the central nervous system of TD. Every significant action emits an event. All consumers (TUI, file logger, database) subscribe to this bus. The event stream becomes the single source of truth.

### How This Relates to Existing Systems

TD already has several messaging systems. Here's how the new EventBus fits:

| System | Purpose | Keep/Replace |
|--------|---------|--------------|
| **StateManager** | Persists execution state to database | **Keep** - still needed for persistence and cross-process state |
| **StateEvent** | Notifies TUI of state changes | **Keep** - still useful for high-level state transitions |
| **Coordinator** | Inter-loop control (stop, rebase, queries) | **Keep** - different concern (control vs observability) |
| **ConversationLog** | Debug logging of LLM conversations | **Replace** - EventBus provides superset |
| **EventBus (NEW)** | Real-time activity streaming | **Add** - new capability |

The EventBus doesn't replace existing systems - it adds a new dimension: **real-time observability**.

```
┌─────────────────────────────────────────────────────────────┐
│                       EVENT BUS                              │
│            (tokio::sync::broadcast channel)                  │
│                                                              │
│  Every action emits an event. Every consumer subscribes.    │
└─────────────────────────────────────────────────────────────┘
        ↑               ↑               ↑               ↑
   LLM Client      Tool Executor    Validation      Loop Engine
   emits:          emits:           emits:          emits:
   - PromptSent    - ToolStarted    - Started       - PhaseStarted
   - TokenDelta    - ToolCompleted  - OutputLine    - IterationStarted
   - ResponseDone  - ToolFailed     - Completed     - IterationCompleted

        ↓               ↓               ↓               ↓
┌───────────┐   ┌───────────┐   ┌───────────┐   ┌───────────┐
│ TUI Live  │   │ File Log  │   │ Database  │   │ Metrics   │
│ Streaming │   │ .jsonl    │   │ (history) │   │ (future)  │
└───────────┘   └───────────┘   └───────────┘   └───────────┘
```

### Architecture

#### Event Types

```rust
/// Core event enum - the vocabulary of TD's activity
#[derive(Clone, Debug, Serialize)]
pub enum TdEvent {
    // === Loop Lifecycle ===
    LoopStarted {
        execution_id: String,
        loop_type: LoopType,
        task_description: String,
    },
    PhaseStarted {
        execution_id: String,
        phase_index: usize,
        phase_name: String,
        total_phases: usize,
    },
    IterationStarted {
        execution_id: String,
        iteration: u32,
    },
    IterationCompleted {
        execution_id: String,
        iteration: u32,
        outcome: IterationOutcome,
    },
    LoopCompleted {
        execution_id: String,
        success: bool,
        total_iterations: u32,
    },

    // === LLM Interactions ===
    PromptSent {
        execution_id: String,
        iteration: u32,
        prompt_summary: String,  // First 200 chars or structured summary
        token_count: u64,
    },
    TokenReceived {
        execution_id: String,
        iteration: u32,
        token: String,
    },
    ResponseCompleted {
        execution_id: String,
        iteration: u32,
        response_summary: String,
        input_tokens: u64,
        output_tokens: u64,
        has_tool_calls: bool,
    },

    // === Tool Execution ===
    ToolCallStarted {
        execution_id: String,
        iteration: u32,
        tool_name: String,
        tool_args_summary: String,
    },
    ToolCallCompleted {
        execution_id: String,
        iteration: u32,
        tool_name: String,
        success: bool,
        result_summary: String,
        duration_ms: u64,
    },

    // === Validation ===
    ValidationStarted {
        execution_id: String,
        iteration: u32,
        command: String,
    },
    ValidationOutput {
        execution_id: String,
        iteration: u32,
        line: String,
        is_stderr: bool,
    },
    ValidationCompleted {
        execution_id: String,
        iteration: u32,
        exit_code: i32,
        duration_ms: u64,
    },

    // === Errors & Warnings ===
    Error {
        execution_id: String,
        context: String,
        message: String,
    },
    Warning {
        execution_id: String,
        context: String,
        message: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub enum IterationOutcome {
    ValidationPassed,
    ValidationFailed { exit_code: i32 },
    MaxTurnsReached,
    ToolError { tool: String, error: String },
    LlmError { error: String },
}
```

#### Event Bus Implementation

```rust
use tokio::sync::broadcast;
use std::sync::Arc;

pub struct EventBus {
    tx: broadcast::Sender<TdEvent>,
    // Configuration
    channel_capacity: usize,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx, channel_capacity: capacity }
    }

    /// Emit an event to all subscribers
    pub fn emit(&self, event: TdEvent) {
        // Ignore send errors (no subscribers is OK)
        let _ = self.tx.send(event);
    }

    /// Subscribe to receive events
    pub fn subscribe(&self) -> broadcast::Receiver<TdEvent> {
        self.tx.subscribe()
    }
}

/// Handle for components to emit events without owning the bus
#[derive(Clone)]
pub struct EventEmitter {
    tx: broadcast::Sender<TdEvent>,
    execution_id: String,
}

impl EventEmitter {
    pub fn emit(&self, event: TdEvent) {
        let _ = self.tx.send(event);
    }

    // Convenience methods
    pub fn prompt_sent(&self, iteration: u32, summary: &str, tokens: u64) {
        self.emit(TdEvent::PromptSent {
            execution_id: self.execution_id.clone(),
            iteration,
            prompt_summary: summary.to_string(),
            token_count: tokens,
        });
    }

    pub fn token_received(&self, iteration: u32, token: &str) {
        self.emit(TdEvent::TokenReceived {
            execution_id: self.execution_id.clone(),
            iteration,
            token: token.to_string(),
        });
    }

    // ... more convenience methods
}
```

#### Integration Points

**1. Loop Engine (`loop/engine.rs`)**

```rust
impl LoopEngine {
    pub async fn run(&mut self, emitter: EventEmitter) -> Result<LoopOutcome> {
        emitter.emit(TdEvent::LoopStarted {
            execution_id: self.exec_id.clone(),
            loop_type: self.loop_type,
            task_description: self.task.clone(),
        });

        for (phase_idx, phase) in self.phases.iter().enumerate() {
            emitter.emit(TdEvent::PhaseStarted {
                execution_id: self.exec_id.clone(),
                phase_index: phase_idx,
                phase_name: phase.name.clone(),
                total_phases: self.phases.len(),
            });

            self.run_phase(phase, &emitter).await?;
        }

        // ... loop completion
    }

    async fn run_agentic_loop(&mut self, emitter: &EventEmitter) -> Result<()> {
        emitter.emit(TdEvent::IterationStarted {
            execution_id: self.exec_id.clone(),
            iteration: self.iteration,
        });

        // Switch from complete() to stream() for LLM calls
        let response = self.llm.stream_with_events(request, emitter).await?;

        // ... rest of iteration
    }
}
```

**2. LLM Client Integration**

The existing `LlmClient` trait already has both `complete()` and `stream()` methods. The RALPH loop currently uses `complete()` (line 654 in `loop/engine.rs`). The change involves:

1. Adding an `EventEmitter` parameter to the agentic loop
2. Wrapping the existing `stream()` method to emit events

```rust
// In loop/engine.rs - modify run_agentic_loop to use streaming
async fn run_agentic_loop(
    &mut self,
    initial_prompt: &str,
    tool_ctx: &ToolContext,
    tool_defs: &[ToolDefinition],
    emitter: &EventEmitter,  // NEW parameter
) -> eyre::Result<AgenticLoopResult> {
    // ... existing setup ...

    loop {
        // Emit prompt sent event
        emitter.emit(TdEvent::PromptSent {
            execution_id: self.exec_id.clone(),
            iteration: self.iteration,
            prompt_summary: truncate(&messages.last().unwrap().content_text(), 200),
            token_count: 0,  // Will be known after response
        });

        // Use streaming instead of complete()
        let (chunk_tx, mut chunk_rx) = mpsc::channel(100);
        let stream_task = self.llm.stream(request.clone(), chunk_tx);

        // Forward chunks to event bus while collecting response
        let mut full_text = String::new();
        tokio::spawn(async move {
            while let Some(chunk) = chunk_rx.recv().await {
                if let StreamChunk::TextDelta(text) = &chunk {
                    emitter.emit(TdEvent::TokenReceived {
                        execution_id: self.exec_id.clone(),
                        iteration: self.iteration,
                        token: text.clone(),
                    });
                }
            }
        });

        let response = stream_task.await?;

        emitter.emit(TdEvent::ResponseCompleted {
            execution_id: self.exec_id.clone(),
            iteration: self.iteration,
            response_summary: truncate(&full_text, 200),
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
            has_tool_calls: !response.tool_calls.is_empty(),
        });

        // ... rest of loop unchanged ...
    }
}
```

**3. Validation Runner (`loop/validation.rs`)**

```rust
pub async fn run_validation_streaming(
    command: &str,
    workdir: &Path,
    timeout: Duration,
    emitter: &EventEmitter,
    iteration: u32,
) -> Result<ValidationResult> {
    emitter.emit(TdEvent::ValidationStarted {
        execution_id: emitter.execution_id.clone(),
        iteration,
        command: command.to_string(),
    });

    let mut child = Command::new("sh")
        .args(["-c", command])
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Stream stdout
    let stdout_emitter = emitter.clone();
    let stdout_task = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            stdout_emitter.emit(TdEvent::ValidationOutput {
                execution_id: stdout_emitter.execution_id.clone(),
                iteration,
                line: line.clone(),
                is_stderr: false,
            });
            output.push_str(&line);
            output.push('\n');
        }
        output
    });

    // Similar for stderr...

    let status = tokio::time::timeout(timeout, child.wait()).await??;
    let stdout_output = stdout_task.await?;
    let stderr_output = stderr_task.await?;

    emitter.emit(TdEvent::ValidationCompleted {
        execution_id: emitter.execution_id.clone(),
        iteration,
        exit_code: status.code().unwrap_or(-1),
        duration_ms: start.elapsed().as_millis() as u64,
    });

    Ok(ValidationResult {
        exit_code: status.code().unwrap_or(-1),
        stdout: stdout_output,
        stderr: stderr_output,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}
```

**4. TUI Runner (`tui/runner.rs`)**

```rust
impl TuiRunner {
    pub async fn run(&mut self, event_bus: Arc<EventBus>) -> Result<()> {
        let mut event_rx = event_bus.subscribe();

        loop {
            tokio::select! {
                // Existing terminal events
                Some(event) = self.events.next() => {
                    self.handle_terminal_event(event).await?;
                }

                // NEW: Event bus events (real-time)
                Ok(td_event) = event_rx.recv() => {
                    self.handle_td_event(td_event).await?;
                }

                // Existing state events (keep for cross-process)
                Ok(state_event) = self.state_event_rx.recv() => {
                    self.handle_state_event(state_event).await?;
                }
            }

            self.render().await?;
        }
    }

    async fn handle_td_event(&mut self, event: TdEvent) -> Result<()> {
        match event {
            TdEvent::TokenReceived { execution_id, token, .. } => {
                // Append token to live output buffer
                if self.is_viewing_execution(&execution_id) {
                    self.state.append_live_output(&token);
                }
            }
            TdEvent::ValidationOutput { execution_id, line, is_stderr, .. } => {
                // Append validation line to logs
                if self.is_viewing_execution(&execution_id) {
                    self.state.append_log_line(&line, is_stderr);
                }
            }
            TdEvent::IterationCompleted { execution_id, iteration, outcome, .. } => {
                // Update iteration status display
                self.state.update_iteration_status(&execution_id, iteration, &outcome);
            }
            // ... handle other events
            _ => {}
        }
        Ok(())
    }
}
```

**5. File Logger**

```rust
pub struct EventLogger {
    writer: BufWriter<File>,
}

impl EventLogger {
    pub fn new(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self { writer: BufWriter::new(file) })
    }

    pub async fn run(mut self, event_bus: Arc<EventBus>) -> Result<()> {
        let mut rx = event_bus.subscribe();

        while let Ok(event) = rx.recv().await {
            let entry = EventLogEntry {
                timestamp: Utc::now(),
                event,
            };
            writeln!(self.writer, "{}", serde_json::to_string(&entry)?)?;
            self.writer.flush()?;
        }
        Ok(())
    }
}
```

### Data Model

#### File Structure

```
~/.taskdaemon/
├── runs/
│   └── {execution-id}/
│       ├── events.jsonl          # All events for this execution
│       ├── iterations/
│       │   └── {n}/
│       │       ├── prompt.txt    # Full prompt sent (optional)
│       │       ├── response.txt  # Full response received (optional)
│       │       ├── stdout.log    # Validation stdout
│       │       └── stderr.log    # Validation stderr
│       └── metadata.json         # Execution metadata
```

#### Event Log Format (events.jsonl)

```json
{"ts":"2026-01-21T10:00:00Z","event":{"type":"LoopStarted","execution_id":"abc123","loop_type":"Phase","task":"Add auth"}}
{"ts":"2026-01-21T10:00:01Z","event":{"type":"IterationStarted","execution_id":"abc123","iteration":1}}
{"ts":"2026-01-21T10:00:02Z","event":{"type":"PromptSent","execution_id":"abc123","iteration":1,"prompt_summary":"Implement OAuth...","token_count":1500}}
{"ts":"2026-01-21T10:00:03Z","event":{"type":"TokenReceived","execution_id":"abc123","iteration":1,"token":"I'll"}}
{"ts":"2026-01-21T10:00:03Z","event":{"type":"TokenReceived","execution_id":"abc123","iteration":1,"token":" implement"}}
...
```

### API Design

#### Public API

```rust
// Create event bus (typically at app startup)
let event_bus = Arc::new(EventBus::new(10000));

// Get emitter for a specific execution
let emitter = event_bus.emitter_for("execution-123");

// Subscribe to events
let mut rx = event_bus.subscribe();

// Filter events for specific execution
let filtered = event_bus.subscribe_filtered(|e| e.execution_id() == "execution-123");
```

### Implementation Plan

#### Phase 1: Core Event Infrastructure
**Files to create:**
- `td/src/events/mod.rs` - Module root
- `td/src/events/types.rs` - `TdEvent` enum definition
- `td/src/events/bus.rs` - `EventBus` and `EventEmitter` implementation
- `td/src/events/logger.rs` - `EventLogger` for file persistence

**Files to modify:**
- `td/src/lib.rs` - Add `pub mod events;`

**Deliverable:** EventBus can be created, events can be emitted and received, events are logged to JSONL files.

#### Phase 2: Loop Engine Integration
**Files to modify:**
- `td/src/loop/engine.rs` - Add `EventEmitter` field, emit lifecycle events
- `td/src/loop/mod.rs` - Re-export emitter if needed

**Key changes:**
- `LoopEngine::new()` takes optional `EventEmitter`
- `run()` emits `LoopStarted`, `IterationStarted`, `IterationCompleted`, `LoopCompleted`
- Thread emitter to `execute_tools()`

**Deliverable:** Running a loop emits lifecycle events visible in log files.

#### Phase 3: LLM Streaming
**Files to modify:**
- `td/src/loop/engine.rs` - Change `run_agentic_loop()` to use `stream()` instead of `complete()`

**Key changes:**
- Line 654: `self.llm.complete(request)` → streaming equivalent
- Spawn task to forward `StreamChunk::TextDelta` to `TdEvent::TokenReceived`
- Collect full response while streaming

**Deliverable:** LLM responses stream token-by-token to event bus.

#### Phase 4: Validation Streaming
**Files to modify:**
- `td/src/loop/validation.rs` - Add `run_validation_streaming()` or modify `run_validation()`

**Key changes (addresses [Analysis Doc](../output-handling-analysis.md) subprocess blocking issue):**
- Switch from `Command::output()` to `Command::spawn()` with piped streams
- Use `tokio::io::BufReader` and `AsyncBufReadExt::lines()` for line-by-line reading
- Emit `ValidationOutput` for each line
- This adopts the otto TeeWriter pattern identified in the analysis

**Deliverable:** Validation output streams line-by-line to event bus.

#### Phase 5: TUI Integration
**Files to modify:**
- `td/src/tui/runner.rs` - Subscribe to event bus, handle events in select loop
- `td/src/tui/state.rs` - Add `live_output: String` buffer, `live_logs: Vec<LogEntry>`
- `td/src/tui/views.rs` - Update `[o]` and `[l]` views to show streaming content

**Key changes:**
- Add `event_rx: broadcast::Receiver<TdEvent>` to TuiRunner
- Add `handle_td_event()` method
- `[o] Output` shows `state.live_output` when execution is active
- `[l] Logs` appends lines from `ValidationOutput` events

**Deliverable:** TUI shows live streaming output as events arrive.

#### Phase 6: Polish
- Add event filtering (by execution, by type)
- Add replay capability from event log
- Performance tuning (batch file writes, backpressure)
- Remove or deprecate `ConversationLog` (replaced by EventLogger)

## Alternatives Considered

### Alternative 1: Extend Existing StateEvent System

- **Description:** Add more event types to the existing `StateEvent` enum and broadcast channel
- **Pros:** Minimal new infrastructure; reuses existing patterns
- **Cons:** StateEvent is designed for state changes, not streaming; would require significant expansion; conflates two concerns (state persistence vs activity streaming)
- **Why not chosen:** The existing system is optimized for polling-based state sync, not real-time activity streaming. Adding streaming would compromise its simplicity.

### Alternative 2: WebSocket-based Event Stream

- **Description:** Expose events via WebSocket for external consumers
- **Pros:** Language-agnostic; enables external tooling; web UI possible
- **Cons:** Added complexity; network overhead for local use; requires server component
- **Why not chosen:** Over-engineered for the current use case. Can be added later as an event bus subscriber if needed.

### Alternative 3: File Tailing Only (No In-Memory Bus)

- **Description:** Write all events to files; TUI tails the files
- **Pros:** Simple; persistent by default; works across processes
- **Cons:** Latency from disk I/O; complexity of file watching; harder to filter
- **Why not chosen:** File I/O latency would compromise the real-time goal. Broadcast channels provide sub-millisecond delivery.

### Alternative 4: Keep Polling, Reduce Interval

- **Description:** Reduce polling interval from 250ms to 50ms
- **Pros:** Minimal code changes
- **Cons:** Still not true streaming; increased CPU usage; doesn't solve the fundamental visibility problem
- **Why not chosen:** Polling can never provide true streaming. The architecture needs to be push-based.

## Technical Considerations

### Dependencies

**Internal:**
- `tokio::sync::broadcast` - already in use for `StateEvent`
- `tokio::sync::mpsc` - already in use for streaming chunks
- `serde` - already in use for serialization
- Existing `LlmClient::stream()` method - already implemented in `AnthropicClient`
- Existing `StreamChunk` enum - already defined with `TextDelta`, `ToolUseStart`, etc.

**External (new):**
- `tokio::io::AsyncBufReadExt` - for line-by-line streaming of validation output (may already be available)

### Performance

**Channel Capacity:**
- 10,000 events recommended for channel capacity
- At ~100 tokens/second, this provides ~100 seconds of buffer
- Slow subscribers will miss events (acceptable for TUI)

**File I/O:**
- Events buffered before write (BufWriter)
- Flush after each event for durability
- Consider async file I/O if write latency becomes issue

**Memory:**
- Each event ~200-500 bytes
- TUI should maintain bounded buffer (e.g., last 1000 events)
- Older events available via file replay

### Security

- Event logs may contain sensitive data (prompts, responses)
- Files written to user-owned directory with standard permissions
- No network exposure in this design

### Testing Strategy

1. **Unit tests:** Event serialization/deserialization
2. **Integration tests:** Event bus pub/sub with multiple subscribers
3. **End-to-end tests:** Full loop with event capture and verification
4. **Performance tests:** High-throughput event emission (1000+ events/sec)

### Rollout Plan

1. Implement behind feature flag initially
2. Enable for new executions only
3. Verify no performance regression
4. Remove flag, make default

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Event bus backpressure overwhelms TUI | Medium | Medium | Bounded buffer in TUI; drop oldest events |
| File I/O slows down loop execution | Low | Medium | Async writes; separate I/O task |
| Breaking change to LLM client interface | Low | Low | `LlmClient::stream()` already exists; no interface change needed |
| Event log files grow unbounded | Medium | Low | Implement rotation; archive old runs |
| Streaming adds complexity to error handling | Medium | Medium | Timeout handling; graceful degradation to batch |
| Coordinator and EventBus overlap | Medium | Low | EventBus is for observability; Coordinator is for inter-loop control. Keep separate concerns. |
| TUI joins late, misses events | Medium | Medium | Read from file log on startup; handle `Lagged` errors gracefully |
| Validation produces massive output | Low | Medium | Add configurable line limit with truncation warning |

## Edge Cases and Failure Modes

### What happens when...

**1. TUI starts after loop is already running?**
- Broadcast channels only deliver events to subscribers who are listening at emit time
- **Solution:** On TUI startup, read `events.jsonl` to reconstruct current state, then subscribe for live updates

**2. Multiple loops run concurrently?**
- Each emits to the same bus with different `execution_id`
- TUI filters by selected execution
- **No issue** - this is the expected behavior

**3. Loop crashes mid-iteration?**
- Events already emitted are persisted to file
- No `IterationCompleted` or `LoopCompleted` event
- **Solution:** TUI can detect incomplete executions; file log provides forensics

**4. Token rate exceeds channel capacity?**
- Slow subscribers lag behind, eventually hit channel capacity
- `broadcast::Receiver::recv()` returns `RecvError::Lagged(n)` - subscriber missed n events
- **Solution:** TUI handles `Lagged` by catching up from file or accepting missed tokens (acceptable for display)

**5. Validation command produces massive output?**
- Each line becomes a `ValidationOutput` event
- Could flood channel and logs
- **Solution:** Add configurable line limit; truncate after N lines with warning event

**6. EventBus created but no subscribers?**
- `broadcast::Sender::send()` returns error when no receivers
- Current design ignores this (`let _ = self.tx.send(event)`)
- **No issue** - events still go to file logger (if running)

**7. File logger can't write (disk full, permissions)?**
- Logger task would error and potentially die
- Loop continues unaffected (fire-and-forget model)
- **Solution:** Logger should log errors to tracing, continue best-effort

**8. User switches between executions rapidly in TUI?**
- Need to clear/switch live buffers
- Historical view should come from file, not memory
- **Solution:** Clear `live_output` on execution switch; populate from file for historical

## Open Questions

- [ ] Should we emit full prompts/responses to events or just summaries?
  - *Recommendation:* Summaries in events (for display), full content to separate files (for debugging)
- [ ] What's the right channel capacity for the event bus?
  - *Recommendation:* 10,000 events (~100 seconds at 100 tokens/sec)
- [ ] Should the TUI auto-follow new executions or require manual selection?
  - *Recommendation:* Auto-follow if only one active, require selection if multiple
- [ ] How long should we retain event log files?
  - *Recommendation:* Same as execution retention policy; clean up with execution
- [ ] Should events include correlation IDs for distributed tracing?
  - *Recommendation:* Not now; `execution_id + iteration` provides sufficient correlation

## Success Criteria

This design is successful when:

1. **User can watch decomposition** - Task → Specs → Phases visible in real-time
2. **User can see LLM thinking** - Token-by-token response streaming in TUI
3. **User can see tool execution** - Tool calls and results appear as they happen
4. **User can see why iteration continues** - Validation output streams, exit code visible
5. **History is preserved** - All events in JSONL files for debugging and replay
6. **No performance regression** - Loop execution speed unchanged (within 5%)

## References

- [Output Handling Analysis](../output-handling-analysis.md) - Comparison of otto and TD approaches
- [Otto TeeWriter implementation](~/repos/otto-rs/otto) - Reference for streaming output
- [tokio broadcast channel docs](https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html)
- Existing TD files:
  - `td/src/llm/client.rs` - `LlmClient` trait with `stream()` method
  - `td/src/llm/types.rs` - `StreamChunk` enum
  - `td/src/loop/engine.rs:654` - Current `complete()` call to replace
  - `td/src/loop/validation.rs` - Current `run_validation()` with `.output()`
  - `td/src/tui/runner.rs` - TUI event loop to extend
  - `td/src/state/messages.rs` - `StateEvent` enum (for reference)
