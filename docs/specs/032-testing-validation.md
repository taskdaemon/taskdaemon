# Spec: End-to-End Testing and Validation

**Status:** Draft
**Created:** 2026-01-19 15:30:00
**ID:** 019bd8-loop-spec-032-testing-validation

## Summary

Perform comprehensive end-to-end testing of the howdy binary to ensure all requirements from the Plan are met, including specific test scenarios and edge cases.

## Problem Statement

Before considering the implementation complete, we need to thoroughly test all functionality, edge cases, and requirements specified in the Plan. This includes testing default behavior, custom greetings, error handling, and various input scenarios.

## Goals

- Test all scenarios mentioned in the Plan
- Verify error handling and exit codes
- Test Unicode support and edge cases
- Validate TTY detection and color output
- Ensure binary works as standalone executable
- Document any limitations or issues found

## Non-Goals

- Performance benchmarking
- Stress testing or load testing
- Security auditing
- Platform-specific testing beyond Linux/macOS

## Acceptance Criteria

1. All test scenarios from the Plan pass:
   - Default greeting shows "howdy"
   - Custom greeting "Hello, World!" works
   - Empty string "" defaults to "howdy"
   - Unicode "👋" displays correctly
2. Colors appear in terminal but not when piping
3. Error messages go to stderr with exit code 1
4. Binary runs without Rust toolchain
5. All edge cases handled gracefully

## Implementation Plan

### Phase 1: Core Functionality Tests
Create test_scenarios.sh:
```bash
#!/bin/bash
set -e

BINARY=~/tmp/howdy
PASS=0
FAIL=0

echo "=== Howdy E2E Test Suite ==="
echo

# Helper function
test_case() {
    local name="$1"
    local cmd="$2"
    local expected="$3"
    local expect_fail="${4:-false}"
    
    echo -n "Testing: $name... "
    
    if [ "$expect_fail" = "true" ]; then
        if ! $cmd 2>/dev/null; then
            echo "PASS (expected failure)"
            ((PASS++))
        else
            echo "FAIL (expected to fail)"
            ((FAIL++))
        fi
    else
        if output=$($cmd 2>&1) && [[ "$output" == *"$expected"* ]]; then
            echo "PASS"
            ((PASS++))
        else
            echo "FAIL"
            echo "  Expected: $expected"
            echo "  Got: $output"
            ((FAIL++))
        fi
    fi
}

# Test 1: Default greeting
test_case "Default greeting" "$BINARY" "howdy"

# Test 2: Custom greeting
test_case "Custom greeting" "$BINARY --greeting 'Hello, World!'" "Hello, World!"

# Test 3: Short flag
test_case "Short flag" "$BINARY -g Hi" "Hi"

# Test 4: Empty string defaults to howdy
test_case "Empty string" "$BINARY --greeting ''" "howdy"

# Test 5: Whitespace only
test_case "Whitespace only" "$BINARY --greeting '   '" "howdy"

# Test 6: Unicode emoji
test_case "Unicode emoji" "$BINARY --greeting 👋" "👋"

# Test 7: Unicode text
test_case "Unicode text" "$BINARY --greeting 'Здравствуйте'" "Здравствуйте"

# Test 8: Multi-word greeting
test_case "Multi-word" "$BINARY --greeting 'Hello there, friend!'" "Hello there, friend!"

echo
echo "Results: $PASS passed, $FAIL failed"
```

### Phase 2: TTY and Color Detection Tests
Create test_tty.sh:
```bash
#!/bin/bash

BINARY=~/tmp/howdy

echo "=== TTY and Color Tests ==="
echo

# Test direct terminal output (should have colors)
echo "1. Direct terminal output (should see green):"
$BINARY --greeting "Color Test"

# Test piped output (should not have colors)
echo -e "\n2. Piped output (should be plain):"
$BINARY --greeting "Pipe Test" | cat

# Test redirected output
echo -e "\n3. Redirected output (should be plain):"
$BINARY --greeting "Redirect Test" > /tmp/howdy_test.txt
cat /tmp/howdy_test.txt
rm -f /tmp/howdy_test.txt

# Test NO_COLOR environment variable
echo -e "\n4. NO_COLOR set (should be plain):"
NO_COLOR=1 $BINARY --greeting "No Color Test"

# Test broken pipe
echo -e "\n5. Broken pipe test (should exit gracefully):"
$BINARY --greeting "Broken Pipe Test" | head -n 0 && echo "Handled gracefully" || echo "Exit code: $?"
```

### Phase 3: Error Handling Tests
Create test_errors.sh:
```bash
#!/bin/bash

BINARY=~/tmp/howdy

echo "=== Error Handling Tests ==="
echo

# Test help flag
echo "1. Help flag:"
$BINARY --help | head -n 5

# Test version flag
echo -e "\n2. Version flag:"
$BINARY --version

# Test invalid flag
echo -e "\n3. Invalid flag (should show error):"
$BINARY --invalid-flag 2>&1 | head -n 3

# Test with closed stdout (simulate write error)
echo -e "\n4. Closed stdout simulation:"
exec 3>&1
exec 1>&-
$BINARY 2>&3 | true
EXIT_CODE=$?
exec 1>&3
exec 3>&-
echo "Exit code for closed stdout: $EXIT_CODE"
```

### Phase 4: Binary Independence Test
Create test_standalone.sh:
```bash
#!/bin/bash

BINARY=~/tmp/howdy

echo "=== Standalone Binary Tests ==="
echo

# Show binary info
echo "1. Binary information:"
file $BINARY
ls -lh $BINARY

# Show dependencies
echo -e "\n2. Binary dependencies:"
if command -v ldd >/dev/null 2>&1; then
    ldd $BINARY
elif command -v otool >/dev/null 2>&1; then
    otool -L $BINARY
fi

# Test without cargo in PATH
echo -e "\n3. Running without Rust toolchain:"
(
    PATH=/usr/bin:/bin
    unset CARGO_HOME
    unset RUSTUP_HOME
    $BINARY --greeting "No Rust Required!"
)

# Test from different directories
echo -e "\n4. Running from various directories:"
cd /
$BINARY --greeting "From root"
cd /tmp
$BINARY --greeting "From /tmp"
cd ~
$BINARY --greeting "From home"
```

### Phase 5: Comprehensive Test Runner
Create run_all_tests.sh:
```bash
#!/bin/bash

echo "Running Howdy Comprehensive Test Suite"
echo "======================================"
date
echo

# Check binary exists
if [ ! -x ~/tmp/howdy ]; then
    echo "ERROR: Binary not found or not executable at ~/tmp/howdy"
    exit 1
fi

# Run all test suites
./test_scenarios.sh
echo -e "\n---\n"
./test_tty.sh
echo -e "\n---\n"
./test_errors.sh
echo -e "\n---\n"
./test_standalone.sh

echo -e "\n======================================"
echo "Test suite completed"
date
```

## Dependencies

- **028-project-setup**: Project must exist
- **029-library-implementation**: Library must be implemented
- **030-cli-implementation**: CLI must be implemented
- **031-build-installation**: Binary must be built and installed

## Risks and Mitigations

- **Risk**: Terminal-specific behavior varies
  - **Mitigation**: Test on multiple terminal emulators if possible
- **Risk**: Unicode support depends on system locale
  - **Mitigation**: Document locale requirements, test with UTF-8
- **Risk**: Color detection may fail in some environments
  - **Mitigation**: Test various TERM values, document limitations

## Testing Strategy

1. Automated test scripts for repeatability
2. Manual verification of color output
3. Edge case testing for robustness
4. Cross-platform testing if available
5. Documentation of any found limitations

## Notes

- Some tests require manual observation (color verification)
- Broken pipe behavior may vary by shell
- Unicode rendering depends on terminal font support
- Exit codes should follow POSIX conventions