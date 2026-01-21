# Spec: Build and Installation

**Status:** Draft
**Created:** 2026-01-19 15:25:00
**ID:** 019bd8-loop-spec-031-build-installation

## Summary

Build the howdy binary in release mode with optimizations and install it to ~/tmp/howdy as a standalone executable that can run without the Rust toolchain.

## Problem Statement

The final step requires building an optimized release binary and placing it in the ~/tmp directory with proper executable permissions. The binary must be self-contained and able to run on systems without Rust installed.

## Goals

- Build the project in release mode with size optimizations
- Copy the built binary to ~/tmp/howdy
- Set executable permissions (755) on the binary
- Verify the binary runs without Rust toolchain
- Ensure binary size is reasonable (<10MB)
- Clean up build artifacts if needed

## Non-Goals

- Creating system packages (deb, rpm, etc.)
- Installing to system directories (/usr/local/bin)
- Creating installer scripts
- Cross-compilation for other platforms

## Acceptance Criteria

1. Release binary builds successfully with `cargo build --release`
2. Binary is copied to ~/tmp/howdy (not ~/tmp/howdy/howdy)
3. Binary has executable permissions (chmod +x)
4. Binary runs when called directly: ~/tmp/howdy
5. Binary size is under 10MB
6. Binary works without cargo or rustc in PATH

## Implementation Plan

### Phase 1: Build Release Binary
```bash
cd ~/tmp/howdy

# Build with release profile (uses optimizations from Cargo.toml)
cargo build --release

# Verify binary was created
ls -lh target/release/howdy

# Check binary size
du -h target/release/howdy
```

### Phase 2: Install Binary
```bash
# Copy binary to ~/tmp (parent directory)
cp target/release/howdy ~/tmp/

# Set executable permissions
chmod 755 ~/tmp/howdy

# Verify installation
ls -la ~/tmp/howdy
```

### Phase 3: Test Standalone Execution
```bash
# Test without cargo in PATH
cd ~/tmp
./howdy
./howdy --help
./howdy --version
./howdy -g "Installation test"

# Test from different directory
cd /tmp
~/tmp/howdy --greeting "Path test"

# Test piping still works
~/tmp/howdy | cat  # Should not show colors
```

### Phase 4: Verify Binary Properties
```bash
# Check binary dependencies (should be minimal)
ldd ~/tmp/howdy  # On Linux
otool -L ~/tmp/howdy  # On macOS

# Verify it's stripped
file ~/tmp/howdy  # Should show "stripped"

# Final size check
ls -lh ~/tmp/howdy  # Should be under 10MB
```

### Phase 5: Create Verification Script
Create verify.sh in project:
```bash
#!/bin/bash
set -e

echo "Verifying howdy installation..."

# Check binary exists
if [ ! -f ~/tmp/howdy ]; then
    echo "ERROR: Binary not found at ~/tmp/howdy"
    exit 1
fi

# Check executable
if [ ! -x ~/tmp/howdy ]; then
    echo "ERROR: Binary is not executable"
    exit 1
fi

# Test execution
echo "Testing default greeting:"
~/tmp/howdy

echo -e "\nTesting custom greeting:"
~/tmp/howdy -g "Verification complete!"

echo -e "\nTesting help:"
~/tmp/howdy --help | head -n 5

# Check size
SIZE=$(du -k ~/tmp/howdy | cut -f1)
if [ $SIZE -gt 10240 ]; then
    echo "WARNING: Binary size ${SIZE}KB exceeds 10MB"
else
    echo "Binary size: ${SIZE}KB (OK)"
fi

echo -e "\nVerification passed!"
```

## Dependencies

- **028-project-setup**: Project must be created
- **029-library-implementation**: Library code must be complete
- **030-cli-implementation**: CLI must be fully implemented

## Risks and Mitigations

- **Risk**: Binary is too large despite optimizations
  - **Mitigation**: Review dependencies, consider additional optimization flags
- **Risk**: Binary has dynamic dependencies that aren't available
  - **Mitigation**: Rust typically produces static binaries, verify with ldd/otool
- **Risk**: Permission issues writing to ~/tmp
  - **Mitigation**: Check permissions first, create directory if needed
- **Risk**: Build fails due to missing dependencies
  - **Mitigation**: Ensure all cargo dependencies are downloaded first

## Testing Strategy

1. Build verification:
   - Confirm release build completes without errors
   - Verify optimization flags are applied

2. Installation testing:
   - Verify file is copied correctly
   - Check permissions are set

3. Execution testing:
   - Run all flag combinations
   - Test from various directories
   - Verify piping behavior

4. System testing:
   - Test on system without Rust installed (if possible)
   - Verify no unexpected dependencies

## Notes

- The release profile in Cargo.toml includes aggressive size optimizations
- Using `strip = true` removes debug symbols automatically
- Binary should be completely self-contained thanks to Rust's static linking
- The binary name matches the package name from Cargo.toml