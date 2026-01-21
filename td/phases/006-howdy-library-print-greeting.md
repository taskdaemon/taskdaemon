---
id: 006-howdy-library-print-greeting
name: Implement howdy::print_greeting (colored + error handling)
spec: 019bd8-loop-spec-029-library-implementation
status: ready
deps:
  - 005-howdy-project-setup
---

# Phase: Implement howdy::print_greeting (colored + error handling)

## Summary
Implement `howdy::print_greeting` in `src/lib.rs` to print a greeting in green when writing to a TTY, defaulting to `"howdy"` for empty/whitespace input, with robust error context for stdout write/flush failures.

## Acceptance Criteria
1. `print_greeting(greeting: &str) -> eyre::Result<()>`:
   - Uses `greeting.trim()` to detect empty/whitespace-only and falls back to `"howdy"`.
   - Writes exactly one line ending with `\n` to stdout.
2. Uses the `colored` crate (`.green()`) and relies on its automatic TTY / `NO_COLOR` behavior (no manual TTY probing required).
3. Stdout write and flush failures return `Err` with useful context via `eyre::WrapErr`.
4. Unit tests exist and pass for defaulting behavior and basic success paths.

## Files to Create/Modify
- Modify: `~/tmp/howdy/src/lib.rs`
- (Optional) Modify: `~/tmp/howdy/Cargo.toml` (if additional dev-deps needed)

## Implementation Notes
- Prefer `writeln!(stdout.lock(), ...)` over `println!` to surface IO errors.
- Keep behavior simple: always apply `.green()`; `colored` will decide whether to emit ANSI codes.

## Test Cases
### Unit tests (cargo test)
1. `print_greeting("")` returns `Ok(())`.
2. `print_greeting("   \t\n")` returns `Ok(())`.
3. `print_greeting("Hello")` returns `Ok(())`.
4. `print_greeting("👋")` returns `Ok(())`.

### Manual
1. `cargo run -- -g "Color"` shows green text in a terminal.
2. `cargo run -- -g "NoColor" | cat` outputs plain text (no ANSI escapes visible).
3. `NO_COLOR=1 cargo run -- -g "NoColor"` outputs plain text.
