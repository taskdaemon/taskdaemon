# Spec: Core Library Implementation

**Status:** Draft
**Created:** 2026-01-19 15:15:00
**ID:** 019bd8-loop-spec-029-library-implementation

## Summary

Implement the core library functionality for the howdy CLI tool, specifically the `print_greeting` function that handles colored output to stdout with proper error handling and TTY detection.

## Problem Statement

The howdy tool needs a library function that can print colored text to stdout while properly handling various edge cases like piped output, TTY detection, and write failures. The colored crate's automatic TTY detection must be leveraged to ensure colors are only used when appropriate.

## Goals

- Implement `print_greeting(greeting: &str) -> Result<()>` function in lib.rs
- Use the colored crate to print green text by default
- Handle empty or whitespace-only input by using "howdy" as default
- Properly handle stdout write failures with contextual errors
- Ensure TTY detection works correctly (no colors when piping)
- Add appropriate error context using eyre

## Non-Goals

- Complex color customization or themes
- Multiple output formats
- Logging or debug output
- Performance optimization beyond reasonable defaults

## Acceptance Criteria

1. `print_greeting` function prints colored output when called with valid input
2. Empty or whitespace-only strings default to "howdy"
3. Colors are automatically disabled when output is piped
4. Write failures to stdout return appropriate errors with context
5. Function includes proper documentation and examples
6. All error paths are handled gracefully

## Implementation Plan

### Phase 1: Basic Implementation
Update src/lib.rs:
```rust
//! The howdy library provides colored greeting output functionality.

use colored::*;
use eyre::{Result, WrapErr};
use std::io::{self, Write};

/// Prints a colored greeting message to stdout.
/// 
/// The greeting is printed in green when outputting to a terminal.
/// Colors are automatically disabled when piping to another process.
/// 
/// # Arguments
/// * `greeting` - The greeting message to print. If empty or only whitespace,
///                defaults to "howdy".
/// 
/// # Returns
/// * `Result<()>` - Ok if successful, Err if stdout write fails
/// 
/// # Examples
/// ```no_run
/// use howdy::print_greeting;
/// 
/// // Print default greeting
/// print_greeting("").unwrap();
/// 
/// // Print custom greeting
/// print_greeting("Hello, World!").unwrap();
/// ```
pub fn print_greeting(greeting: &str) -> Result<()> {
    let greeting = if greeting.trim().is_empty() {
        "howdy"
    } else {
        greeting
    };
    
    // Use colored crate which handles TTY detection automatically
    let colored_greeting = greeting.green();
    
    // Print to stdout with explicit flush
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    
    writeln!(handle, "{}", colored_greeting)
        .wrap_err("Failed to write greeting to stdout")?;
    
    handle.flush()
        .wrap_err("Failed to flush stdout")?;
    
    Ok(())
}
```

### Phase 2: Add Tests
Add to src/lib.rs:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_greeting() {
        // Test with empty string
        assert!(print_greeting("").is_ok());
        
        // Test with whitespace
        assert!(print_greeting("   ").is_ok());
        assert!(print_greeting("\t\n").is_ok());
    }
    
    #[test]
    fn test_custom_greeting() {
        assert!(print_greeting("Hello, World!").is_ok());
        assert!(print_greeting("👋").is_ok());
        assert!(print_greeting("Multi\nLine").is_ok());
    }
}
```

### Phase 3: Error Handling Improvements
Enhance error messages:
```rust
use std::io::ErrorKind;

// In print_greeting function, enhance error handling:
writeln!(handle, "{}", colored_greeting)
    .wrap_err_with(|| {
        match io::Error::last_os_error().kind() {
            ErrorKind::BrokenPipe => "Output pipe was closed",
            ErrorKind::PermissionDenied => "Permission denied writing to stdout",
            _ => "Failed to write greeting to stdout",
        }
    })?;
```

### Phase 4: Documentation and Examples
Add comprehensive documentation:
```rust
//! # Howdy Library
//! 
//! A simple library for printing colored greetings to the terminal.
//! 
//! ## Features
//! 
//! - Automatic TTY detection - colors only appear in terminals
//! - Graceful handling of pipe closures
//! - Unicode support for international greetings
//! - Minimal dependencies and small binary size
//! 
//! ## Usage
//! 
//! ```no_run
//! use howdy::print_greeting;
//! 
//! fn main() -> eyre::Result<()> {
//!     // Use default greeting
//!     print_greeting("")?;
//!     
//!     // Use custom greeting
//!     print_greeting("Bonjour!")?;
//!     
//!     Ok(())
//! }
//! ```
```

## Dependencies

- **028-project-setup**: Requires the project to be created and dependencies configured

## Risks and Mitigations

- **Risk**: BrokenPipe errors when piping to commands like `head`
  - **Mitigation**: Catch and handle BrokenPipe specifically, exit gracefully
- **Risk**: Terminal color support varies across platforms
  - **Mitigation**: Rely on colored crate's built-in detection and fallback
- **Risk**: Unicode handling issues on some terminals
  - **Mitigation**: Test with various Unicode inputs, document limitations

## Testing Strategy

1. Unit tests for greeting logic and default handling
2. Integration test that captures stdout
3. Manual testing:
   - Direct terminal output: `cargo run`
   - Piped output: `cargo run | cat` (should have no colors)
   - Broken pipe: `cargo run | head -n 0`
   - Unicode: Test with emojis and international characters

## Notes

- The colored crate automatically handles NO_COLOR and TERM environment variables
- Using writeln! instead of println! for better error handling
- Explicit stdout locking improves performance for multiple writes
- flush() ensures output appears immediately, important for interactive use