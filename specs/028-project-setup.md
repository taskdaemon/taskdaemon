# Spec: Project Setup and Directory Structure

**Status:** Draft
**Created:** 2026-01-19 15:10:00
**ID:** 019bd8-loop-spec-028-project-setup

## Summary

Create a new Rust project for the "howdy" CLI binary with proper directory structure, dependency configuration, and initial project setup. This includes creating the ~/tmp directory if needed, initializing a Cargo project, and configuring the Cargo.toml with required dependencies and build optimizations.

## Problem Statement

Before implementing the howdy CLI tool, we need a properly structured Rust project with all necessary dependencies configured. The project needs specific build settings for creating an optimized standalone binary and must be set up in the ~/tmp/howdy directory.

## Goals

- Create ~/tmp directory if it doesn't exist
- Initialize new Cargo project named "howdy" in ~/tmp/howdy
- Configure Cargo.toml with required dependencies: clap 4.x with derive, eyre 0.6, colored 2.x
- Set up release profile with optimizations (LTO, codegen-units=1, strip)
- Create lib.rs and main.rs structure for library/binary separation
- Ensure project builds successfully with cargo check

## Non-Goals

- Implementation of actual functionality
- Testing setup beyond basic project structure
- CI/CD configuration
- Documentation beyond basic comments

## Acceptance Criteria

1. ~/tmp/howdy directory exists with a valid Cargo project
2. Cargo.toml contains all required dependencies with correct versions
3. Release profile is configured with size optimizations
4. Both src/lib.rs and src/main.rs files exist with minimal boilerplate
5. Project passes `cargo check` without errors
6. .gitignore is configured for Rust projects

## Implementation Plan

### Phase 1: Directory and Project Creation
```bash
# Create directory structure
mkdir -p ~/tmp
cd ~/tmp
cargo new howdy --bin

# Verify creation
cd howdy
ls -la
```

### Phase 2: Configure Dependencies
Update Cargo.toml:
```toml
[package]
name = "howdy"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4.5", features = ["derive"] }
eyre = "0.6"
colored = "2.1"

[[bin]]
name = "howdy"
path = "src/main.rs"

[lib]
name = "howdy"
path = "src/lib.rs"

[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Enable Link Time Optimization
codegen-units = 1   # Single codegen unit for better optimization
strip = true        # Strip symbols from binary
panic = "abort"     # Smaller panic handler
```

### Phase 3: Create Initial File Structure
Create src/lib.rs:
```rust
//! The howdy library provides colored greeting output functionality.

use eyre::Result;

/// Prints a colored greeting message to stdout.
/// 
/// # Arguments
/// * `greeting` - The greeting message to print
/// 
/// # Returns
/// * `Result<()>` - Ok if successful, Err if stdout write fails
pub fn print_greeting(greeting: &str) -> Result<()> {
    todo!("Implement greeting function")
}
```

Create src/main.rs:
```rust
//! A simple CLI tool for printing colored greetings.

use clap::Parser;
use eyre::Result;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The greeting message to display
    #[arg(short, long, default_value = "howdy")]
    greeting: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    
    // TODO: Call library function
    println!("Greeting: {}", args.greeting);
    
    Ok(())
}
```

### Phase 4: Verify Setup
```bash
# Check project structure
tree src/

# Verify it builds
cargo check

# Test that dependencies resolve
cargo tree
```

## Dependencies

None - this is the first spec in the implementation sequence.

## Risks and Mitigations

- **Risk**: ~/tmp directory creation fails due to permissions
  - **Mitigation**: Check permissions first, provide clear error message if creation fails
- **Risk**: Cargo.toml syntax errors prevent building
  - **Mitigation**: Use exact TOML syntax provided, validate with cargo check
- **Risk**: Dependency versions become incompatible
  - **Mitigation**: Pin to specific minor versions that are known to work together

## Testing Strategy

Manual verification:
1. Confirm directory structure matches expected layout
2. Run `cargo check` to ensure no compilation errors
3. Verify all dependencies are listed in Cargo.lock
4. Check that both lib and bin targets are recognized by Cargo

## Notes

- The project uses both lib.rs and main.rs to properly separate concerns
- Release optimizations are aggressive to minimize binary size
- The initial implementations use todo!() macros as placeholders