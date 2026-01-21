---
id: 007-howdy-cli-wireup
name: Implement howdy CLI (clap args + error handling)
spec: 019bd8-loop-spec-030-cli-implementation
status: ready
deps:
  - 005-howdy-project-setup
  - 006-howdy-library-print-greeting
---

# Phase: Implement howdy CLI (clap args + error handling)

## Summary
Implement `src/main.rs` to parse `--greeting/-g` via clap derive, call `howdy::print_greeting`, and handle failures by printing a user-friendly message to stderr and exiting non-zero (except for broken pipe).

## Acceptance Criteria
1. `howdy --greeting <value>` and `howdy -g <value>` both work.
2. Default greeting is `"howdy"` when the flag is absent.
3. `--help` and `--version` work via clap.
4. On error from `print_greeting`:
   - message is printed to stderr
   - process exits with code `1`
5. If the error is a broken pipe (`io::ErrorKind::BrokenPipe`), the program exits with code `0` and does not print a noisy error.
6. Integration tests exist (using `assert_cmd` + `predicates`) and pass.

## Files to Create/Modify
- Modify: `~/tmp/howdy/src/main.rs`
- Create: `~/tmp/howdy/tests/cli.rs`
- Modify: `~/tmp/howdy/Cargo.toml` (add dev-deps: `assert_cmd`, `predicates`)

## Test Cases
### Integration tests (cargo test)
1. `howdy` prints `howdy` to stdout and exits 0.
2. `howdy --greeting "Hello, World!"` prints `Hello, World!` and exits 0.
3. `howdy -g Hi` prints `Hi` and exits 0.
4. `howdy --help` exits 0 and output mentions `greeting`.
5. `howdy --version` exits 0 and output contains `howdy`.

### Manual
1. `cargo run -- --greeting "Howdy"`
2. `cargo run -- --greeting "Pipe" | head -n 0` exits successfully (0) without error spam.
