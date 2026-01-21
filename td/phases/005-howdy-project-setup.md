---
id: 005-howdy-project-setup
name: Howdy project setup (Cargo + deps + layout)
spec: 019bd8-loop-spec-028-project-setup
status: ready
deps: []
---

# Phase: Howdy project setup (Cargo + deps + layout)

## Summary
Create a new Rust workspace-free Cargo project at `~/tmp/howdy` with the required dependencies, release profile optimizations, and initial `lib.rs`/`main.rs` layout.

## Acceptance Criteria
1. A Cargo project exists at `~/tmp/howdy` and `cargo metadata` recognizes both a `lib` and `bin` target.
2. `Cargo.toml` includes dependencies:
   - `clap` 4.x with `derive` feature
   - `eyre` 0.6
   - `colored` 2.x
3. `Cargo.toml` includes a `[profile.release]` section with:
   - `opt-level = "z"`
   - `lto = true`
   - `codegen-units = 1`
   - `strip = true`
   - `panic = "abort"`
4. `src/lib.rs` exists and exports `pub fn print_greeting(greeting: &str) -> eyre::Result<()>` (may be `todo!()` for now).
5. `src/main.rs` exists and uses `clap` derive to parse `--greeting/-g` with default `howdy` (may print placeholder output for now).
6. `cargo check` succeeds in `~/tmp/howdy`.

## Files to Create/Modify
- Create: `~/tmp/howdy/` (Cargo project)
- Modify: `~/tmp/howdy/Cargo.toml`
- Create/Modify: `~/tmp/howdy/src/lib.rs`
- Create/Modify: `~/tmp/howdy/src/main.rs`
- Create/Modify: `~/tmp/howdy/.gitignore`

## Implementation Notes
- Use `cargo new howdy --bin` in `~/tmp`.
- Add `[lib]` and `[[bin]]` entries so both targets are explicit.

## Test Cases
### Manual/CLI
1. `cd ~/tmp/howdy && cargo check`
2. `cd ~/tmp/howdy && cargo run -- --help` (help text renders)
3. `cd ~/tmp/howdy && cargo run -- --version` (prints version)
4. `cd ~/tmp/howdy && cargo run -- -g "Hi"` (runs and prints something non-empty)
