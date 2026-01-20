# Spec: CLI Binary Implementation

**Status:** Draft
**Created:** 2026-01-19 15:20:00
**ID:** 019bd8-loop-spec-030-cli-implementation

## Summary

Implement the CLI binary (main.rs) that uses clap for argument parsing and integrates with the library function to provide a complete command-line interface for the howdy tool.

## Problem Statement

The howdy tool needs a CLI interface that accepts command-line arguments, specifically the `--greeting` flag, and handles errors appropriately by printing to stderr and setting correct exit codes. The implementation must use clap's derive API for clean, maintainable code.

## Goals

- Implement CLI argument parsing using clap derive
- Wire main.rs to call the library's print_greeting function
- Handle errors by printing to stderr and exiting with code 1
- Support --greeting/-g flag with "howdy" as default
- Include --help and --version functionality via clap
- Ensure clean error messages for end users

## Non-Goals

- Subcommands or complex CLI structure
- Configuration file parsing
- Interactive mode or prompts
- Shell completion generation

## Acceptance Criteria

1. Binary accepts `--greeting <value>` or `-g <value>` arguments
2. Default greeting is "howdy" when flag is not provided
3. `--help` displays usage information
4. `--version` displays version from Cargo.toml
5. Errors are printed to stderr with exit code 1
6. Success exits with code 0
7. Binary works correctly with the library function

## Implementation Plan

### Phase 1: Complete CLI Structure
Update src/main.rs:
```rust
//! A simple CLI tool for printing colored greetings.

use clap::Parser;
use eyre::Result;
use howdy::print_greeting;

/// Print colored greetings to the terminal
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The greeting message to display
    #[arg(short, long, default_value = "howdy")]
    greeting: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    print_greeting(&args.greeting)?;
    Ok(())
}
```

### Phase 2: Enhanced Error Handling
Improve error messages for common cases:
```rust
use std::io;

fn main() {
    if let Err(e) = run() {
        // Check for broken pipe specifically
        if let Some(io_err) = e.downcast_ref::<io::Error>() {
            if io_err.kind() == io::ErrorKind::BrokenPipe {
                // Exit silently for broken pipe
                std::process::exit(0);
            }
        }
        
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
```

### Phase 3: Add Metadata
Update CLI metadata for better help output:
```rust
/// Print colored greetings to the terminal
/// 
/// This tool prints a greeting message in color when outputting to a terminal.
/// Colors are automatically disabled when piping to other commands.
/// 
/// Examples:
///   howdy                    # prints "howdy" in green
///   howdy -g "Hello!"        # prints "Hello!" in green
///   howdy --greeting "Hi"    # prints "Hi" in green
#[derive(Parser)]
#[command(
    name = "howdy",
    author,
    version,
    about = "A friendly greeting tool",
    long_about = None,
    after_help = "ENVIRONMENT:\n    \
        NO_COLOR    Set to disable colored output\n    \
        TERM        Terminal type affects color support"
)]
struct Args {
    /// The greeting message to display
    /// 
    /// If empty or only whitespace, defaults to "howdy"
    #[arg(
        short,
        long,
        default_value = "howdy",
        help = "Custom greeting message",
        long_help = "The greeting message to display.\n\
                     If empty or only whitespace, defaults to \"howdy\"."
    )]
    greeting: String,
}
```

### Phase 4: Integration Tests
Create tests/integration_test.rs:
```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_default_greeting() {
    let mut cmd = Command::cargo_bin("howdy").unwrap();
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("howdy"));
}

#[test]
fn test_custom_greeting() {
    let mut cmd = Command::cargo_bin("howdy").unwrap();
    cmd.arg("--greeting").arg("Hello, World!");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Hello, World!"));
}

#[test]
fn test_short_flag() {
    let mut cmd = Command::cargo_bin("howdy").unwrap();
    cmd.arg("-g").arg("Hi");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Hi"));
}

#[test]
fn test_help() {
    let mut cmd = Command::cargo_bin("howdy").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("greeting"));
}

#[test]
fn test_version() {
    let mut cmd = Command::cargo_bin("howdy").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("howdy"));
}
```

## Dependencies

- **028-project-setup**: Requires project structure and dependencies
- **029-library-implementation**: Requires the print_greeting function to be implemented

## Risks and Mitigations

- **Risk**: Clap parsing errors confuse users
  - **Mitigation**: Clap provides good default error messages, test edge cases
- **Risk**: Exit codes don't follow conventions
  - **Mitigation**: Explicitly handle success (0) and error (1) cases
- **Risk**: Broken pipe errors appear as failures
  - **Mitigation**: Detect and handle broken pipe separately

## Testing Strategy

1. Unit tests for argument parsing logic
2. Integration tests using assert_cmd:
   - Default behavior
   - Custom greetings
   - Help and version flags
   - Error cases
3. Manual testing:
   - Various greeting values
   - Empty strings
   - Unicode characters
   - Piping scenarios

## Notes

- Using separate run() function makes testing easier
- Clap's derive API provides automatic help and version generation
- The binary name in Cargo.toml determines the executable name
- Exit code conventions: 0 for success, 1 for general errors