# Design Document: Extract Conversation Module from Runner

**Author:** Claude (with Scott)
**Date:** 2026-01-17
**Status:** Deferred
**Review Passes:** 5/5

## Summary

Extract LLM conversation handling from `runner.rs` (~500 lines) into a new `conversation.rs` module. This reduces `runner.rs` from 1303 lines to ~800 lines and creates a focused module for all LLM streaming, tool execution, and conversation state management.

## Problem Statement

### Background

The TUI runner (`src/tui/runner.rs`) currently handles multiple responsibilities:
- Terminal management and event loop
- State synchronization with StateManager
- LLM streaming and conversation management
- Tool execution
- Action execution (cancel/pause/resume)
- Plan draft creation

This conflation makes it difficult to:
1. Understand the LLM conversation flow in isolation
2. Test conversation logic independently
3. Find relevant code when debugging REPL issues

### Problem

`runner.rs` at 1303 lines is too large and handles too many concerns. The LLM conversation handling (~500 lines) is a distinct responsibility that should be isolated.

### Goals

- Extract LLM conversation handling into `src/tui/conversation.rs`
- Reduce `runner.rs` to ~800 lines focused on orchestration
- Enable independent testing of conversation logic
- Maintain identical runtime behavior (pure refactor)

### Non-Goals

- Splitting views into separate files (future work)
- Changing the conversation/streaming architecture
- Adding new features to the REPL
- Modifying the LLM client interface

## Proposed Solution

### Overview

Create a new `Conversation` struct that owns all LLM-related state and methods. The `TuiRunner` will hold an instance of `Conversation` and delegate REPL operations to it.

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      TuiRunner                          │
│  - terminal: Tui                                        │
│  - event_handler: EventHandler                          │
│  - state_manager: Option<StateManager>                  │
│  - conversation: Conversation  ◄── NEW                  │
│                                                         │
│  Methods:                                               │
│  - run() - main loop                                    │
│  - handle_tick() - delegates to conversation            │
│  - refresh_data() - state manager sync                  │
│  - execute_action() - cancel/pause/resume               │
└─────────────────────────────────────────────────────────┘
                          │
                          │ delegates REPL operations
                          ▼
┌─────────────────────────────────────────────────────────┐
│                     Conversation                        │
│  - llm_client: Option<Arc<dyn LlmClient>>              │
│  - tool_executor: ToolExecutor                          │
│  - worktree: PathBuf                                    │
│  - messages: Vec<Message>  (LLM conversation)           │
│  - chat_system_prompt: String                           │
│  - plan_system_prompt: String                           │
│  - stream_rx: Option<Receiver<StreamChunk>>            │
│  - llm_result_rx: Option<Receiver<LlmTaskResult>>      │
│  - llm_task: Option<JoinHandle<()>>                    │
│                                                         │
│  Methods:                                               │
│  - new(worktree, llm_client) -> Self                   │
│  - submit(input, mode, app_state) - start request      │
│  - poll(app_state) - process chunks/results            │
│  - is_streaming() -> bool                               │
│  - clear() - reset conversation                         │
└─────────────────────────────────────────────────────────┘
```

### Data Model

The `Conversation` struct encapsulates:

```rust
/// Manages LLM conversation state and streaming
pub struct Conversation {
    /// LLM client for API calls
    llm_client: Option<Arc<dyn LlmClient>>,
    /// Tool executor for running tools
    tool_executor: ToolExecutor,
    /// Working directory for tool context
    worktree: PathBuf,
    /// LLM conversation history (API format, not display)
    messages: Vec<Message>,
    /// System prompt for Chat mode
    chat_system_prompt: String,
    /// System prompt for Plan mode
    plan_system_prompt: String,
    /// Receiver for streaming chunks
    stream_rx: Option<mpsc::Receiver<StreamChunk>>,
    /// Receiver for LLM task results
    llm_result_rx: Option<mpsc::Receiver<LlmTaskResult>>,
    /// Handle to background LLM task
    llm_task: Option<JoinHandle<()>>,
}

/// Result from background LLM task (moved from runner.rs)
#[derive(Debug)]
pub enum LlmTaskResult {
    Response {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        stop_reason: StopReason,
    },
    Error(String),
}
```

### API Design

```rust
impl Conversation {
    /// Create new conversation manager
    pub fn new(worktree: PathBuf, llm_client: Option<Arc<dyn LlmClient>>) -> Self;

    /// Submit user input, starting an LLM request
    /// - Adds user message to app_state.repl_history
    /// - Spawns background LLM task
    /// - Sets app_state.repl_streaming = true
    /// Returns error message if cannot start (e.g., already streaming, no client)
    pub fn submit(&mut self, input: &str, mode: ReplMode, app_state: &mut AppState) -> Option<String>;

    /// Poll for streaming updates - call on each tick
    /// This method has two parts:
    /// 1. poll_chunks() - sync, drains stream_rx into app_state.repl_response_buffer
    /// 2. poll_results() - async, processes LlmTaskResult, executes tools if needed
    /// Updates app_state with new content, handles tool calls
    pub async fn poll(&mut self, app_state: &mut AppState);

    /// Check if currently streaming
    pub fn is_streaming(&self) -> bool;

    /// Clear conversation history (for /clear command)
    pub fn clear(&mut self);

    /// Generate a short title from text (for plan naming)
    /// Used by runner.rs for create_plan_draft()
    pub async fn generate_title(&self, text: &str) -> Option<String>;
}
```

**Note on sync vs async:** The current `process_stream_chunks()` is synchronous (uses `try_recv()`), while `process_llm_results()` is async (tool execution). The `poll()` method combines both - the chunk processing remains sync within the async method.

**Architecture Note:** `Conversation` takes `&mut AppState` rather than owning display state or using channels. This creates coupling but is pragmatic:
- TUI is single-threaded async (one task, not parallel)
- Avoids channel complexity for UI updates
- Matches the existing pattern in `runner.rs`
- Display state (`repl_history`, `repl_streaming`) lives in `AppState` for view rendering

### Implementation Plan

**Phase 1: Create conversation.rs with struct and basic methods**
1. Create `src/tui/conversation.rs`
2. Move `LlmTaskResult` enum
3. Define `Conversation` struct with all fields
4. Implement `new()` constructor
5. Move `build_chat_system_prompt()` and `build_plan_system_prompt()`
6. Add module to `src/tui/mod.rs`

**Phase 2: Move streaming logic**
1. Move `process_stream_chunks()` → `poll_stream_chunks()`
2. Move `finish_streaming()` → internal method
3. Move `is_streaming()` helper

**Phase 3: Move LLM request handling**
1. Move `start_repl_request()` → `submit()`
2. Move `continue_llm_request()` → internal `continue_request()`
3. Move `get_tool_definitions()` → internal method

**Phase 4: Move tool and result handling**
1. Move `handle_tool_calls()` → internal `execute_tools()`
2. Move `process_llm_results()` → integrate into `poll()`

**Phase 5: Move utility methods**
1. Move `generate_title()`
2. Move `current_system_prompt()` → internal method

**Phase 6: Update runner.rs**
1. Replace REPL fields with single `conversation: Conversation`
2. Update `handle_tick()` to call `conversation.poll()`
3. Update REPL submit handling:
   - Check `pending_repl_submit` in `handle_tick()`
   - Call `conversation.submit()` with input
   - `submit()` adds user message to `app_state.repl_history`
4. Remove moved methods
5. Keep `create_plan_draft()` in runner.rs (uses conversation.generate_title() but is plan-workflow specific)

**Phase 7: Testing and cleanup**
1. Verify `cargo check` passes
2. Run existing tests
3. Manual testing of REPL flow
4. Remove any dead code

## Alternatives Considered

### Alternative 1: Trait-based abstraction

- **Description:** Define a `ConversationHandler` trait and implement it
- **Pros:** More flexible for testing with mocks
- **Cons:** Adds complexity, only one implementation needed currently
- **Why not chosen:** YAGNI - we can add traits later if needed for testing

### Alternative 2: Keep in runner.rs but refactor into impl blocks

- **Description:** Use separate `impl` blocks to group related methods
- **Pros:** Less code churn, no new files
- **Cons:** Still a large file, methods can't be truly isolated
- **Why not chosen:** Doesn't achieve the goal of separation of concerns

### Alternative 3: Actor-based conversation handler

- **Description:** Run conversation as separate tokio task communicating via channels
- **Pros:** True isolation, could enable concurrent conversations
- **Cons:** Significant complexity increase, channel overhead
- **Why not chosen:** Over-engineering for current needs

## Technical Considerations

### Dependencies

- Internal: `crate::llm`, `crate::tools`, `super::state`
- External: `tokio` (channels, spawn), `tracing`

### Performance

No performance impact expected - this is a pure refactor moving code between modules. The async patterns and channel usage remain identical.

### Security

No security implications - no changes to tool execution sandboxing or input handling.

### Testing Strategy

1. **Compile-time:** `cargo check` must pass
2. **Unit tests:** Existing tests in `runner.rs` continue to work
3. **Integration:** Manual testing of REPL flow:
   - Start conversation in Chat mode
   - Start conversation in Plan mode
   - Tool execution (list_directory, read_file)
   - Multi-turn conversations
   - /clear command
   - Tab to switch modes

### Rollout Plan

Single commit refactor. No feature flags needed since behavior is unchanged.

## Edge Cases

### LLM Task Panic
If the spawned LLM task panics, the JoinHandle will return an error. Current code ignores the JoinHandle result. The `poll()` method should detect channel closure (rx returns None/error) and clean up streaming state.

### Tool Execution Failure
Current behavior: individual tool errors are returned to the LLM as error results, conversation continues. This behavior is preserved - no change needed.

### Clear While Streaming
If `/clear` is called while streaming:
- `clear()` should call `finish_streaming()` first
- Cancel the background task if possible (or let it complete and ignore results)
- Reset all state

Implementation note: Add `abort()` method or have `clear()` handle this case.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Subtle behavior change | Low | Medium | Careful code movement, comprehensive manual testing |
| Borrow checker issues | Medium | Low | May need to adjust ownership/lifetimes during extraction |
| Missed method dependency | Low | Low | Compiler will catch missing imports/methods |
| Channel closed unexpectedly | Low | Low | Handle None/Err from try_recv gracefully |

## Open Questions

- [x] Should `Conversation` own `AppState` mutations or take `&mut AppState`? → Take `&mut AppState` to avoid ownership complexity
- [ ] Should we add unit tests for `Conversation` in this refactor or defer? → Decide during implementation

## Files Changed

| File | Change |
|------|--------|
| `src/tui/mod.rs` | Add `pub mod conversation;` |
| `src/tui/conversation.rs` | **NEW** - ~400 lines extracted from runner |
| `src/tui/runner.rs` | Remove ~500 lines, add `conversation: Conversation` field |

## References

- Current runner.rs: `src/tui/runner.rs` (1303 lines)
- LLM client interface: `src/llm/mod.rs`
- Tool executor: `src/tools/mod.rs`
