# Design Document: Collapsed Tool Output Display

**Author:** Claude (with Scott)
**Date:** 2026-01-17
**Status:** Implemented
**Review Passes:** 5/5

## Summary

Implement collapsed tool output display in the REPL view, matching Claude Code's behavior. Each tool type gets a custom summary format. Users can expand/collapse with Ctrl+O.

## Problem Statement

### Background

Currently, the REPL displays full tool output inline, which can be very long (e.g., `read_file` showing 50+ lines). This causes important context to scroll off screen.

Claude Code solves this with **tool-aware summaries**:
```
● Bash(wc -l src/tui/runner.rs)
└ 1663 src/tui/runner.rs

● Search(pattern: "fn ", path: "src/", output_mode: "content")
└ Found 34 lines (ctrl+o to expand)

● Update(docs/design/example.md)
└ Added 1 line, removed 1 line
    5  -**Status:** Ready for Review
    5  +**Status:** Deferred
```

### Problem

Tool output dominates the REPL view, making conversation hard to follow.

### Goals

- Tool-aware collapsed summaries (not just truncation)
- Visual format: `● ToolName(args)` header, `└` tree connector for output
- Ctrl+O to expand/collapse
- Short output stays full (no collapse needed)

### Non-Goals

- Syntax highlighting (future)
- Persisting expand/collapse state across sessions

## Proposed Solution

### Overview

1. Add an `expanded` field to `ReplMessage` to track expand/collapse state per message
2. Modify the view rendering to truncate tool results when collapsed
3. Add keybind handling to toggle expansion of the "current" or most recent tool result
4. Show visual indicator for collapsed content with line count

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       AppState                              │
│  repl_history: Vec<ReplMessage>                            │
│       │                                                     │
│       └─► ReplMessage { role, content, timestamp, expanded }│
│                                           ▲                 │
│                                           │                 │
│  repl_selected_msg: Option<usize>  ◄──────┘                │
│  (index of message to expand/collapse)                      │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    render_repl_view()                       │
│                                                             │
│  for (idx, msg) in history.iter().enumerate():              │
│    if msg.is_tool_result() && !msg.expanded:                │
│      render_collapsed_tool(msg)   ◄── NEW                   │
│    else:                                                    │
│      render_full_message(msg)     ◄── existing              │
└─────────────────────────────────────────────────────────────┘
```

### Data Model

Update `ReplMessage`:

```rust
/// REPL message for display
#[derive(Debug, Clone)]
pub struct ReplMessage {
    pub role: ReplRole,
    pub content: String,
    pub timestamp: i64,
    /// Whether tool output is expanded (only relevant for ToolResult)
    pub expanded: bool,
}
```

Add to `AppState`:

```rust
/// Index of currently selected message for expand/collapse (None = auto-select latest tool)
pub repl_selected_tool: Option<usize>,
```

### Rendering Design

Collapsed tool output format (tool result only - "Running..." is streaming):
```
[read_file]  1│[package]
             2│name = "taskdaemon"
             3│version = "0.1.0"
            … +47 lines (o to expand)
```

Expanded format:
```
[read_file]  1│[package]
             2│name = "taskdaemon"
             3│version = "0.1.0"
             4│edition = "2024"
             ... (all lines shown)
            50│# end of file
```

Key design decisions:
- Only tool RESULTS are collapsible (not assistant text or user input)
- Show first 3 content lines with line numbers
- Dim style for collapsed indicator
- Press 'o' to expand/collapse the most recent collapsible tool result

### API Design

```rust
impl ReplMessage {
    /// Check if this is a tool result that can be collapsed
    pub fn is_collapsible(&self) -> bool {
        matches!(self.role, ReplRole::ToolResult { .. })
            && self.content.lines().count() > COLLAPSE_THRESHOLD
    }

    /// Toggle expanded state
    pub fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }
}

impl AppState {
    /// Toggle expand/collapse for latest tool result, or selected one
    pub fn toggle_tool_expansion(&mut self) {
        // Find the latest tool result or use selected index
        let idx = self.repl_selected_tool.unwrap_or_else(|| {
            self.repl_history.iter().rposition(|m| m.is_collapsible())
                .unwrap_or(0)
        });

        if let Some(msg) = self.repl_history.get_mut(idx) {
            msg.toggle_expanded();
        }
    }
}
```

Constants:
```rust
/// Number of lines to show when collapsed
const COLLAPSE_PREVIEW_LINES: usize = 3;

/// Minimum lines before collapsing (don't collapse short output)
const COLLAPSE_THRESHOLD: usize = 6;
```

### Implementation Plan

**Phase 1: Data model updates**
1. Add `expanded: bool` field to `ReplMessage` (default: `false`)
2. Add `repl_selected_tool: Option<usize>` to `AppState`
3. Update constructors to initialize `expanded = false`

**Phase 2: Rendering changes**
1. Create `render_collapsed_tool()` helper function
2. Modify message rendering loop to check `is_collapsible()` and `expanded`
3. Show line numbers in collapsed preview
4. Add "… +N lines (o to expand)" indicator

**Phase 3: Keybind handling**
1. Add 'o' key in normal mode (REPL view) to toggle expansion
2. Add Ctrl+O as alternative keybind
3. Optionally: use Up/Down to select which tool to expand when multiple exist

**Phase 4: Polish**
1. Visual indicator for which tool is "selected" for expansion
2. Auto-expand when scrolling up past collapsed content (optional)
3. Ensure proper scroll adjustment when expanding/collapsing

## Alternatives Considered

### Alternative 1: Always collapsed, no expand option

- **Description:** Tool output always shows first 3 lines, no way to see full output
- **Pros:** Simpler implementation, cleaner UI
- **Cons:** Users can't see full output when needed; would need separate "view logs" feature
- **Why not chosen:** Sometimes users need to see full tool output for debugging

### Alternative 2: Collapse all messages, not just tools

- **Description:** Allow collapsing any long message (assistant responses too)
- **Pros:** More flexible, handles long assistant responses
- **Cons:** More complex, changes conversation readability, assistant responses are usually important
- **Why not chosen:** Tool output is the primary pain point; can extend later if needed

### Alternative 3: Virtual scrolling with lazy render

- **Description:** Only render visible lines, virtualize the scroll buffer
- **Pros:** Better performance for very long conversations
- **Cons:** Much more complex, requires significant refactor of rendering
- **Why not chosen:** Over-engineering for current needs; collapse is simpler and sufficient

## Technical Considerations

### Dependencies

- Internal: `state.rs` (ReplMessage), `views.rs` (rendering), `app.rs` (keybinds)
- No new external dependencies

### Performance

Minimal impact. Counting lines (`content.lines().count()`) is O(n) but tool output is typically <1000 lines. Could cache line count if needed.

### Security

No security implications - this is purely a display feature.

### Testing Strategy

1. **Unit tests:** Test `is_collapsible()` with various line counts
2. **Unit tests:** Test `toggle_expanded()` state changes
3. **Manual testing:**
   - Tool output < threshold shows full
   - Tool output > threshold shows collapsed
   - 'o' key toggles expansion
   - Scroll position adjusts appropriately

### Rollout Plan

Single commit with all changes. No feature flag needed - collapse is a strict UX improvement.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Line count calculation wrong | Low | Low | Unit test edge cases (empty, 1 line, exactly threshold) |
| Scroll position jumps on expand | Medium | Medium | Adjust scroll offset when expanding to keep context visible |
| Keybind conflicts | Low | Low | Use 'o' which is unused in REPL normal mode |
| Confusing which tool is selected | Medium | Low | Add visual highlight for selected tool |

## Edge Cases

### Streaming Tool Output
While a tool is executing and streaming output, show it expanded (live updates). Once complete, collapse it. The `repl_streaming` flag can help detect this state.

### "Running tool..." Status Line
Currently the assistant shows `[calling tool_name]` before the tool executes. This should remain visible and NOT be collapsed - only the tool RESULT gets collapsed.

### Very Short Tool Output
If tool output is ≤ COLLAPSE_THRESHOLD lines (6), show it fully - no collapse indicator needed.

### Empty Tool Output
Some tools might return empty results. Show `[tool_name] (empty result)` rather than collapsing nothing.

### Multiple Tools in Sequence
When multiple tool calls happen in sequence, each is independently collapsible. The 'o' key toggles the most recent one. For more control:
- 'O' (shift+o) could expand ALL tool results
- Future: navigate between tools with '[' and ']' keys

### Manual Scroll Mode
If user has scrolled up (manual scroll mode), 'o' should toggle the tool result nearest to the current scroll position, not the most recent. This keeps the interaction intuitive.

## Open Questions

- [x] Should collapsed be default? → Yes, matches Claude Code behavior
- [x] Should we show line numbers in collapsed preview? → Yes, helps orient user
- [x] Should expand/collapse affect scroll position? → Yes, need to adjust to keep context
- [x] Key binding: 'o', Ctrl+O, or both? → Both for discoverability

## Files Changed

| File | Change |
|------|--------|
| `src/tui/state.rs` | Add `expanded` to ReplMessage, `repl_selected_tool` to AppState |
| `src/tui/views.rs` | Add `render_collapsed_tool()`, modify message loop |
| `src/tui/app.rs` | Add 'o' and Ctrl+O keybind handling |

## References

- Claude Code UI for reference behavior
- Current views.rs rendering: lines 230-280
- Current state.rs ReplMessage: lines 248-290
