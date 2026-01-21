---
id: 009-howdy-e2e-validation
name: End-to-end validation scripts for howdy
spec: 019bd8-loop-spec-032-testing-validation
status: ready
deps:
  - 008-howdy-build-install
---

# Phase: End-to-end validation scripts for howdy

## Summary
Add repeatable E2E validation scripts (or documented command sequences) to verify the installed `~/tmp/howdy` binary across the scenarios in the spec: defaults, custom greeting, unicode, piping/no color, help/version, and common error/broken-pipe behavior.

## Acceptance Criteria
1. A set of runnable scripts exists in the project (e.g. `scripts/`) that can be executed locally to validate behavior against the installed binary at `~/tmp/howdy`.
2. The scripts cover, at minimum:
   - default greeting
   - custom greeting (long and short flag)
   - empty string and whitespace defaulting
   - unicode (emoji + non-latin)
   - piping disables colors (no ANSI escapes) or at least demonstrates expected behavior
   - `--help` and `--version`
   - broken pipe scenario (`| head -n 0`) exits successfully
3. A top-level runner script executes all sub-scripts and returns non-zero if any check fails.

## Files to Create/Modify
- Create: `~/tmp/howdy/scripts/test_scenarios.sh`
- Create: `~/tmp/howdy/scripts/test_tty.sh`
- Create: `~/tmp/howdy/scripts/test_errors.sh`
- Create: `~/tmp/howdy/scripts/test_standalone.sh`
- Create: `~/tmp/howdy/scripts/run_all_tests.sh`
- (Optional) Update: `~/tmp/howdy/README.md` with instructions

## Test Cases
### Scripted
1. `cd ~/tmp/howdy && bash scripts/run_all_tests.sh`

### Expected checks inside scripts
- `~/tmp/howdy` output contains `howdy`
- `~/tmp/howdy --greeting 'Hello, World!'` output contains `Hello, World!`
- `~/tmp/howdy --greeting ''` output contains `howdy`
- `~/tmp/howdy --greeting '👋'` output contains `👋`
- `~/tmp/howdy --greeting 'Здравствуйте'` output contains that string
- `~/tmp/howdy --greeting 'Pipe' | cat` shows plain output (or `NO_COLOR=1` ensures plain)
- `~/tmp/howdy --help` exits 0
- `~/tmp/howdy --version` exits 0
- `~/tmp/howdy --greeting 'Broken' | head -n 0` exits 0
