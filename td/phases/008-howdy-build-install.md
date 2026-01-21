---
id: 008-howdy-build-install
name: Build release binary and install to ~/tmp/howdy
spec: 019bd8-loop-spec-031-build-installation
status: ready
deps:
  - 007-howdy-cli-wireup
---

# Phase: Build release binary and install to ~/tmp/howdy

## Summary
Produce an optimized release build and install it as a standalone executable at `~/tmp/howdy` (note: path is the file itself), ensure permissions, and validate it runs without the Rust toolchain in `PATH`.

## Acceptance Criteria
1. `cargo build --release` succeeds.
2. The resulting binary is copied to `~/tmp/howdy` (a file), not a directory.
3. `~/tmp/howdy` is executable (`chmod 755` or equivalent) and runnable directly.
4. Binary size is under 10MB.
5. Running with a minimal `PATH` (no cargo/rustc) succeeds:
   - `PATH=/usr/bin:/bin ~/tmp/howdy --version` exits 0.

## Files to Create/Modify
- Create: `~/tmp/howdy` (installed binary file)
- (Optional) Create: `~/tmp/howdy/verify.sh` (project-local verification script)

## Test Cases
### Manual/CLI
1. `cd ~/tmp/howdy && cargo build --release`
2. `cp target/release/howdy ~/tmp/howdy && chmod 755 ~/tmp/howdy`
3. `~/tmp/howdy` prints default greeting and exits 0.
4. `~/tmp/howdy -g "Install test"` prints string and exits 0.
5. `ls -lh ~/tmp/howdy` shows size < 10MB.
6. `PATH=/usr/bin:/bin unset CARGO_HOME RUSTUP_HOME; ~/tmp/howdy --help`
