# Design Document: Cross-Process IPC for Daemon Wake-Up

**Author:** Claude (with Scott)
**Date:** 2026-01-21
**Status:** Implemented
**Review Passes Completed:** 5/5

## Summary

The TUI and daemon run as separate processes. When a user activates a draft or toggles execution state in the TUI, the daemon doesn't learn about it until its 60-second poll interval. This document proposes a Unix Domain Socket-based IPC mechanism so the TUI can immediately wake the daemon when work is ready.

## Problem Statement

### Background

TaskDaemon's architecture separates concerns between processes:

```
┌─────────────────────────┐     ┌─────────────────────────┐
│         TUI             │     │        DAEMON           │
│  (td command, user UI)  │     │  (td run-daemon)        │
│                         │     │                         │
│  - StateManager         │     │  - StateManager         │
│  - LLM Client           │     │  - LoopManager          │
│  - Event Bus            │     │  - Scheduler            │
│                         │     │  - Cascade Handler      │
└─────────────────────────┘     └─────────────────────────┘
          │                               │
          └───────── Shared ──────────────┘
                     Storage
              (SQLite via TaskStore)
```

The previous design doc ([2026-01-19-event-driven-work-pickup.md](2026-01-19-event-driven-work-pickup.md)) added `StateEvent::ExecutionPending` to enable immediate work pickup. However, it assumed **in-process** communication via `tokio::sync::broadcast`.

This works perfectly when:
- **Cascade** creates child executions (happens inside the daemon)
- The daemon's StateManager broadcasts → daemon's LoopManager receives → immediate spawn

This **does not work** when:
- **TUI** activates a draft or changes execution state
- TUI's StateManager broadcasts → goes nowhere (TUI's own event bus)
- Daemon's LoopManager doesn't know → waits 60 seconds for poll

### Problem

**30 seconds to 1 hour delay** between user action in TUI and daemon picking up the work.

The current flow:
1. User presses `s` in TUI to start a draft
2. TUI calls `state_manager.activate_draft(id)`
3. TUI's StateManager writes to shared SQLite store
4. TUI's StateManager sends `ExecutionPending` event to **its own** in-process broadcast channel
5. TUI calls `notify_state_change()` which bumps a counter in `~/.local/share/taskdaemon/.state_version`
6. **Daemon doesn't watch this file**
7. Daemon's LoopManager polls every 60 seconds
8. Up to 60 seconds later, daemon finds the pending execution

### Goals

- **Sub-second latency** from TUI action to daemon pickup (target: <100ms)
- **Event-driven** - no polling, no file watching
- **Cross-process** - TUI (process A) notifies daemon (process B)
- **Clean architecture** - use standard IPC patterns, no hacks
- **No new dependencies** - use what tokio provides

### Non-Goals

- Replacing the existing in-process event system (it works for cascade)
- Adding a full RPC framework (gRPC, etc.)
- Supporting Windows named pipes (Linux/macOS focus for now)
- Multi-daemon coordination (single daemon per user)

## Proposed Solution

### Overview

Add a **Unix Domain Socket** listener to the daemon. When the TUI changes execution state, it connects to the socket and sends a simple JSON message. The daemon wakes immediately and processes the request.

```
┌─────────────────────────┐          ┌─────────────────────────┐
│         TUI             │          │        DAEMON           │
│                         │   JSON   │                         │
│  activate_draft() ──────┼──────────▶  socket listener       │
│                         │   over   │       │                 │
│                         │   UDS    │       ▼                 │
│                         │          │  LoopManager.run()      │
│                         │          │  tokio::select! {       │
│                         │          │    client = accept() => │
│                         │          │      handle_ipc_msg()   │
│                         │          │  }                      │
└─────────────────────────┘          └─────────────────────────┘
          │                                    │
          └────────────────────────────────────┘
                    Shared Socket:
           ~/.local/share/taskdaemon/daemon.sock
```

### Architecture

#### Socket Location

```
~/.local/share/taskdaemon/daemon.sock   # Runtime socket (deleted on clean shutdown)
~/.local/share/taskdaemon/taskdaemon.pid  # Existing PID file
~/.local/share/taskdaemon/taskdaemon.version  # Existing version file
```

The socket is created by the daemon on startup and removed on shutdown.

#### Message Protocol

Simple JSON-over-newline protocol. Each message is a single line of JSON followed by `\n`.

```rust
/// Messages from TUI to Daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DaemonMessage {
    /// Notify daemon that an execution is pending and should be picked up
    ExecutionPending { id: String },

    /// Ping to check if daemon is alive
    Ping,

    /// Request daemon to stop gracefully
    Shutdown,
}

/// Responses from Daemon to TUI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DaemonResponse {
    /// Acknowledgment
    Ok,

    /// Pong response to ping
    Pong { version: String },

    /// Error
    Error { message: String },
}
```

#### Daemon Side: Socket Listener

```rust
// In daemon startup (main.rs run_daemon)
let socket_path = get_socket_path();  // ~/.local/share/taskdaemon/daemon.sock

// Clean up stale socket if exists
if socket_path.exists() {
    std::fs::remove_file(&socket_path)?;
}

let listener = UnixListener::bind(&socket_path)?;
info!(?socket_path, "IPC socket listening");

// Pass listener to LoopManager
loop_manager.run(shutdown_rx, listener).await?;
```

```rust
// In LoopManager::run()
pub async fn run(
    &mut self,
    mut shutdown_rx: mpsc::Receiver<()>,
    ipc_listener: UnixListener,  // NEW parameter
) -> Result<()> {
    // ... existing setup ...

    loop {
        tokio::select! {
            // NEW: Handle IPC connections
            Ok((stream, _addr)) = ipc_listener.accept() => {
                if let Err(e) = self.handle_ipc_connection(stream).await {
                    warn!(error = %e, "IPC connection error");
                }
            }

            // Existing: Handle in-process state events (for cascade)
            event = state_events.recv() => {
                match event {
                    Ok(StateEvent::ExecutionPending { id }) => {
                        self.try_spawn_execution(&id).await;
                    }
                    // ... existing handling ...
                }
            }

            // Existing: Fallback polling
            _ = interval.tick() => {
                self.poll_and_spawn().await?;
            }

            // Existing: Shutdown
            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }

    // Cleanup socket on shutdown
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

async fn handle_ipc_connection(&mut self, mut stream: UnixStream) -> Result<()> {
    let mut reader = BufReader::new(&mut stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let msg: DaemonMessage = serde_json::from_str(line.trim())?;

    let response = match msg {
        DaemonMessage::ExecutionPending { id } => {
            debug!(%id, "IPC: ExecutionPending received");
            self.try_spawn_execution(&id).await;
            DaemonResponse::Ok
        }
        DaemonMessage::Ping => {
            DaemonResponse::Pong { version: crate::daemon::VERSION.to_string() }
        }
        DaemonMessage::Shutdown => {
            self.shutdown_requested = true;
            DaemonResponse::Ok
        }
    };

    let response_json = serde_json::to_string(&response)?;
    stream.write_all(response_json.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    Ok(())
}

async fn try_spawn_execution(&mut self, id: &str) {
    if let Ok(Some(exec)) = self.state.get_execution(id).await {
        if self.loop_deps_satisfied(&exec).await.unwrap_or(false) {
            if let Err(e) = self.spawn_loop(&exec).await {
                warn!(%id, error = %e, "Failed to spawn from IPC");
            }
        } else {
            debug!(%id, "IPC: deps not satisfied, will pick up on next poll");
        }
    }
}
```

#### TUI Side: IPC Client

```rust
// New module: src/ipc/client.rs

use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new() -> Self {
        Self {
            socket_path: get_socket_path(),
        }
    }

    /// Notify daemon that an execution is pending
    pub async fn notify_pending(&self, execution_id: &str) -> Result<()> {
        let msg = DaemonMessage::ExecutionPending {
            id: execution_id.to_string()
        };
        self.send_message(msg).await?;
        Ok(())
    }

    /// Check if daemon is alive
    pub async fn ping(&self) -> Result<String> {
        match self.send_message(DaemonMessage::Ping).await? {
            DaemonResponse::Pong { version } => Ok(version),
            _ => Err(eyre::eyre!("Unexpected response")),
        }
    }

    async fn send_message(&self, msg: DaemonMessage) -> Result<DaemonResponse> {
        let mut stream = UnixStream::connect(&self.socket_path).await?;

        let msg_json = serde_json::to_string(&msg)?;
        stream.write_all(msg_json.as_bytes()).await?;
        stream.write_all(b"\n").await?;

        let mut reader = BufReader::new(&mut stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await?;

        let response: DaemonResponse = serde_json::from_str(response_line.trim())?;
        Ok(response)
    }
}
```

#### Integration with StateManager

Modify `StateManager::activate_draft()` to also notify daemon via IPC:

```rust
// In state/manager.rs

pub async fn activate_draft(&self, id: &str) -> StateResponse<()> {
    // ... existing logic to update status ...

    // Notify in-process subscribers (for cascade)
    if result.is_ok() {
        let _ = self.event_tx.send(StateEvent::ExecutionPending { id: exec_id.clone() });
    }

    // Notify daemon via IPC (for TUI -> daemon)
    if result.is_ok() {
        let _ = self.notify_daemon_pending(&exec_id).await;
    }

    result
}

async fn notify_daemon_pending(&self, id: &str) {
    // Fire-and-forget: don't block on IPC errors
    let client = DaemonClient::new();
    if let Err(e) = client.notify_pending(id).await {
        debug!(error = %e, "Could not notify daemon via IPC (may be running in same process)");
    }
}
```

### Data Model

No changes to persisted data. The IPC is purely for real-time notification.

### API Design

#### New Public Types

```rust
// src/ipc/mod.rs
pub mod client;
pub mod messages;

// src/ipc/messages.rs
pub enum DaemonMessage { ... }
pub enum DaemonResponse { ... }

// src/ipc/client.rs
pub struct DaemonClient { ... }
```

#### Internal Changes

| Component | Change |
|-----------|--------|
| `LoopManager::run()` | Accept `UnixListener` parameter, handle in select loop |
| `StateManager::activate_draft()` | Call `notify_daemon_pending()` after success |
| `StateManager::start_draft()` | Call `notify_daemon_pending()` after success |
| `run_daemon()` | Create and bind socket, pass to LoopManager |
| `DaemonManager::stop()` | Could use IPC shutdown message instead of SIGTERM |

### Implementation Plan

#### Phase 1: IPC Module Foundation
**Files to create:**
- `td/src/ipc/mod.rs` - Module root
- `td/src/ipc/messages.rs` - `DaemonMessage` and `DaemonResponse` enums
- `td/src/ipc/client.rs` - `DaemonClient` for TUI side
- `td/src/ipc/listener.rs` - Helper for daemon socket setup

**Files to modify:**
- `td/src/lib.rs` - Add `pub mod ipc;`
- `td/Cargo.toml` - No changes needed (tokio already has UnixStream)

**Deliverable:** Compiles, types defined.

#### Phase 2: Daemon Socket Listener
**Files to modify:**
- `td/src/main.rs` - Create socket in `run_daemon()`, pass to LoopManager
- `td/src/loop/manager.rs` - Accept listener, add to select loop, handle messages

**Deliverable:** Daemon listens on socket, handles ping/pong.

#### Phase 3: TUI Integration
**Files to modify:**
- `td/src/state/manager.rs` - Add IPC notification to `activate_draft()`, `start_draft()`

**Deliverable:** TUI activation immediately wakes daemon.

#### Phase 4: Error Handling & Cleanup
- Handle stale socket files
- Graceful degradation if socket unavailable
- Tests for connection errors

**Deliverable:** Robust error handling.

#### Phase 5: Optional Enhancements
- Use IPC for shutdown instead of SIGTERM
- Add `td status --ping` command using IPC
- Consider connection pooling if high volume

## Alternatives Considered

### Alternative 1: File Watching (inotify/kqueue)

- **Description:** Daemon watches `.state_version` file for changes using `notify` crate
- **Pros:** Cross-platform; no new socket infrastructure
- **Cons:** "Old school" as you said; adds filesystem watching complexity; latency varies by OS; additional dependency
- **Why not chosen:** We explicitly decided against file watching in favor of proper IPC

### Alternative 2: Named Pipe (FIFO)

- **Description:** Create a named pipe; TUI writes "wake up" messages; daemon reads
- **Pros:** Simpler than sockets; one-way communication
- **Cons:** Can't send structured messages easily; no response channel; harder error handling
- **Why not chosen:** Sockets provide bidirectional communication and cleaner protocol

### Alternative 3: D-Bus

- **Description:** Use Linux D-Bus for inter-process communication
- **Pros:** Standard Linux IPC; rich feature set; introspectable
- **Cons:** Heavy dependency; Linux-only; overkill for simple wake-up signal
- **Why not chosen:** Too heavyweight for our needs

### Alternative 4: Shared Memory with Semaphore

- **Description:** Use shared memory region with semaphore for signaling
- **Pros:** Extremely fast; low latency
- **Cons:** Complex to implement correctly; platform-specific; hard to debug
- **Why not chosen:** Unnecessary complexity for this use case

### Alternative 5: HTTP/REST Local Server

- **Description:** Daemon runs local HTTP server; TUI makes HTTP requests
- **Pros:** Language-agnostic; debuggable with curl; could support web UI later
- **Cons:** Heavier than needed; port management; HTTP overhead
- **Why not chosen:** Unix sockets are lighter weight and more appropriate for local IPC

### Alternative 6: `interprocess` Crate

- **Description:** Use the `interprocess` crate for cross-process channels
- **Pros:** Higher-level API; abstracts platform differences
- **Cons:** New dependency; less control; tokio integration unclear
- **Why not chosen:** tokio already has everything we need built-in

## Technical Considerations

### Dependencies

**Internal:**
- `tokio::net::UnixListener`, `UnixStream` - already available in tokio
- `serde`, `serde_json` - already in use
- `eyre` - already in use for error handling

**External (new):**
- None! Everything needed is already in tokio.

### Performance

- **Socket accept:** ~microseconds
- **Message parse:** ~microseconds
- **Total latency:** <1ms from TUI send to daemon receive
- **Comparison:** 60,000ms (current poll) → <1ms (with IPC)

### Security

- Socket is user-owned in user directory
- Standard Unix permissions apply
- No authentication needed (single-user system)
- Socket path in user's home directory, not world-accessible

### Testing Strategy

1. **Unit tests:** Message serialization/deserialization
2. **Integration tests:**
   - Daemon accepts connection, responds to ping
   - TUI sends ExecutionPending, daemon spawns loop
3. **Edge case tests:**
   - Stale socket cleanup
   - Daemon not running (connection refused)
   - Malformed messages

### Rollout Plan

1. Implement in feature branch
2. Test manually: TUI → daemon activation latency
3. Verify fallback: 60s poll still works if IPC fails
4. Merge to main
5. No migration needed - purely additive

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Stale socket file blocks daemon startup | Medium | Low | Check and remove stale socket on startup |
| TUI can't connect (daemon not running) | Medium | Low | Graceful fallback; daemon starts on TUI launch anyway |
| Socket permissions wrong | Low | Medium | Use standard path in user directory |
| Message protocol needs to evolve | Medium | Low | JSON is flexible; add versioning field if needed |
| macOS sandbox restrictions | Low | Medium | Use standard location; test on macOS |
| Race: TUI sends before daemon ready | Low | Low | TUI retries on connection refused; daemon polls as fallback |

## Open Questions

- [ ] Should we support multiple concurrent TUI connections?
  - *Recommendation:* Yes, handle each in separate task
- [ ] Should IPC replace SIGTERM for daemon shutdown?
  - *Recommendation:* Yes, cleaner than signals, implement in Phase 5
- [ ] Should we add a keep-alive / heartbeat?
  - *Recommendation:* No, connections are short-lived request/response
- [ ] Should we add a `td daemon ping` CLI command?
  - *Recommendation:* Yes, useful for debugging, implement in Phase 5

## References

- [tokio UnixListener docs](https://docs.rs/tokio/latest/tokio/net/struct.UnixListener.html)
- [tokio UnixStream docs](https://docs.rs/tokio/latest/tokio/net/struct.UnixStream.html)
- Existing TD files:
  - `td/src/daemon.rs` - DaemonManager, socket path logic
  - `td/src/loop/manager.rs` - LoopManager::run() select loop
  - `td/src/state/manager.rs` - StateManager::activate_draft()
  - `td/src/main.rs:run_daemon()` - Daemon startup

---

## Review Process

### Review Pass 1: Draft (Complete)

Initial draft complete. All sections filled. Ready for correctness review.

**Status:** Proceeding to Pass 2.

---

### Review Pass 2: Correctness (Complete)

Reviewing technical accuracy against actual codebase...

**Issue 1: `LoopManager::run()` signature**
- Current: `pub async fn run(&mut self, mut shutdown_rx: mpsc::Receiver<()>) -> Result<()>`
- Need to modify to accept `UnixListener` parameter
- **Correction applied:** Updated design to show signature change clearly

**Issue 2: `start_draft()` also needs IPC notification**
- `start_draft()` in state/manager.rs:560 sets status to Pending but does NOT send `ExecutionPending` event
- This is a different code path than `activate_draft()` (which does send the event)
- **Correction applied:** Added `start_draft()` to the list of methods needing IPC integration

**Issue 3: `resume_execution()` sets Running directly, not Pending**
- `resume_execution()` sets status to `Running` (not `Pending`)
- Daemon would only pick this up during its 60s poll (looking for orphaned Running executions)
- This is actually a design issue in the current system
- **Decision:** Keep current behavior (Running), but add IPC notification so daemon can immediately spawn task for the resumed execution. Add new message type `ExecutionResumed`.

**Issue 4: Socket path function doesn't exist yet**
- The design references `get_socket_path()` but it doesn't exist
- **Correction applied:** Added socket path helper function to Phase 1, following pattern from `daemon.rs`

**Issue 5: CLI `td start` command also uses `start_draft`**
- `main.rs:626` calls `state.start_draft(&id)` from CLI
- This path also needs IPC notification
- **Correction applied:** IPC notification should be in `StateManager` methods, not TUI-specific code

**Changes made:**
- Added `ExecutionResumed` message type for resume operations
- Updated integration points to include `resume_execution()`
- Clarified that IPC notification lives in StateManager methods
- Added socket path helper to implementation plan

**Updated Message Protocol:**

```rust
/// Messages from TUI/CLI to Daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DaemonMessage {
    /// Notify daemon that an execution is pending and should be picked up
    ExecutionPending { id: String },

    /// Notify daemon that an execution was resumed and should be spawned
    ExecutionResumed { id: String },

    /// Ping to check if daemon is alive
    Ping,

    /// Request daemon to stop gracefully
    Shutdown,
}
```

**Updated Integration Points:**

| Method | Status Change | Event | IPC Message |
|--------|---------------|-------|-------------|
| `activate_draft()` | Draft → Pending | `ExecutionPending` | `ExecutionPending` |
| `start_draft()` | Draft → Pending | (none currently) | `ExecutionPending` |
| `resume_execution()` | Paused → Running | (none) | `ExecutionResumed` |

**Status:** Proceeding to Pass 3.

---

### Review Pass 3: Clarity (Complete)

Reviewing as someone who must implement this without additional context...

**Clarity Issue 1: Socket path helper not shown**
- `get_socket_path()` is referenced but never defined
- **Fix:** Add explicit helper function in the IPC module section

**Clarity Issue 2: Missing import context in code snippets**
- Code snippets use types like `BufReader`, `UnixStream` without showing imports
- **Fix:** Add import statements to key code blocks

**Clarity Issue 3: How does daemon know to NOT send IPC to itself?**
- When cascade creates executions inside daemon, calling `notify_daemon_pending` would connect to itself
- This is wasteful (the in-process event already handles this)
- **Fix:** Add conditional: only send IPC if running in TUI/CLI context, not daemon context
- Or simpler: fire-and-forget means self-connection is harmless (just extra work)
- **Decision:** Document this explicitly - the fire-and-forget approach means daemon connecting to itself is harmless but slightly wasteful. This is acceptable for simplicity.

**Clarity Issue 4: Phase order could be clearer**
- Implementation phases reference files but don't show concrete task list
- **Fix:** Add numbered tasks within each phase

**Changes made:**
- Added socket path helper function definition
- Clarified fire-and-forget behavior for daemon self-connection
- Added numbered subtasks to implementation phases

**Added helper function:**

```rust
// src/ipc/mod.rs

/// Get the socket path for daemon IPC
pub fn get_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("taskdaemon")
        .join("daemon.sock")
}
```

**Updated Phase 1 with numbered tasks:**

1. Create `td/src/ipc/mod.rs` with `get_socket_path()` helper
2. Create `td/src/ipc/messages.rs` with `DaemonMessage` and `DaemonResponse` enums
3. Create `td/src/ipc/client.rs` with `DaemonClient` struct
4. Add `pub mod ipc;` to `td/src/lib.rs`
5. Add unit tests for message serialization

**Status:** Proceeding to Pass 4.

---

### Review Pass 4: Edge Cases (Complete)

Examining failure modes and edge cases...

**Edge Case 1: Daemon not running when TUI sends IPC**
- `UnixStream::connect()` returns `ConnectionRefused`
- **Handling:** Fire-and-forget approach ignores this error silently; daemon will poll eventually
- **Acceptable:** Yes, this is the expected degradation path

**Edge Case 2: Stale socket file from crashed daemon**
- Daemon crashed without cleanup; socket file exists but no process listening
- `connect()` would fail with `ConnectionRefused`
- **Handling:** Same as above - fire-and-forget
- **On daemon startup:** Remove existing socket file before binding

**Edge Case 3: Two daemons started simultaneously**
- First daemon binds socket; second daemon fails to bind
- **Handling:** Already handled by existing PID file logic; second daemon won't start
- **Socket behavior:** Second `bind()` would return `AddrInUse`

**Edge Case 4: TUI and daemon use different socket paths**
- Could happen if `dirs::runtime_dir()` returns different values
- **Mitigation:** Use same helper function everywhere; test on both Linux and macOS

**Edge Case 5: Message too large (malicious or buggy client)**
- `read_line()` could potentially read huge strings
- **Mitigation:** Add size limit to read (e.g., 1KB max for a message)

**Edge Case 6: Daemon busy processing when IPC arrives**
- Daemon is in `spawn_loop()` or `poll_and_spawn()`
- **Handling:** `tokio::select!` will queue the accept; connection waits
- Potential issue: If `spawn_loop` blocks too long, client times out
- **Mitigation:** Consider spawning IPC handler in separate task

**Edge Case 7: Rapid-fire IPC messages (multiple drafts activated quickly)**
- Each activation sends IPC; daemon receives multiple
- **Handling:** Each message handled sequentially; `try_spawn_execution` is idempotent
- **Acceptable:** Yes, idempotency ensures correctness

**Edge Case 8: IPC arrives between execution creation and status update**
- Race: TUI writes to DB, IPC arrives, daemon reads old status
- **Timeline:**
  1. TUI: `update_execution(status=Pending)` - writes to SQLite
  2. TUI: `notify_daemon_pending(id)` - sends IPC
  3. Daemon: receives IPC, calls `get_execution(id)`
- SQLite is synchronous, so write completes before IPC send
- **Acceptable:** Race is prevented by SQLite synchronous writes

**Changes made:**
- Added size limit recommendation for message reading
- Added note about spawning IPC handler in separate task for responsiveness
- Documented idempotency guarantee

**Added to Risks table:**

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Large/malicious message DoS | Low | Low | Add 1KB message size limit |
| Long-running spawn blocks IPC | Medium | Low | Spawn IPC handler in separate task |

**Status:** Proceeding to Pass 5.

---

### Review Pass 5: Excellence (Complete)

Final review for quality, consistency, and ship-readiness...

**Excellence Check 1: Is the solution appropriately simple?**
- Yes: Uses existing tokio primitives, no new deps
- Yes: Simple JSON-over-newline protocol
- Yes: Fire-and-forget keeps TUI fast

**Excellence Check 2: Does it fit the existing codebase patterns?**
- Yes: Follows existing module structure (`td/src/ipc/`)
- Yes: Uses same error handling patterns (eyre)
- Yes: Uses same serialization (serde_json)
- Yes: Matches daemon.rs pattern for path helpers

**Excellence Check 3: Is the fallback robust?**
- Yes: 60-second polling still works if IPC fails entirely
- Yes: In-process events still work for cascade

**Excellence Check 4: Are there unnecessary complexities?**
- Question: Do we need bidirectional communication?
- Answer: Yes for Ping/Pong (health check) and future shutdown via IPC
- Question: Should we version the protocol?
- Answer: Not initially; JSON is flexible enough. Add `"version": 1` field if needed later.

**Excellence Check 5: Documentation complete?**
- Yes: Problem clearly stated
- Yes: Solution well-diagrammed
- Yes: Implementation phases concrete
- Yes: Edge cases covered
- Minor: Add sequence diagram for typical flow

**Final change: Add sequence diagram**

```
Sequence: TUI activates draft

TUI                     StateManager              Daemon (LoopManager)
 |                           |                           |
 |--activate_draft(id)------>|                           |
 |                           |--update SQLite----------->|
 |                           |--send ExecutionPending--->| (in-process, ignored by TUI)
 |                           |--connect to daemon.sock---|
 |                           |--send {"type":"ExecutionPending","id":"..."}-->|
 |                           |                           |--try_spawn_execution()
 |                           |<--{"type":"Ok"}-----------|
 |<--Ok----------------------|                           |
 |                           |                           |
```

**Document status:** Ready for implementation.

---

## Final Status

**Document complete after 5 review passes.**

- Pass 1: Draft - all sections filled
- Pass 2: Correctness - fixed 5 technical issues
- Pass 3: Clarity - added missing definitions, numbered tasks
- Pass 4: Edge Cases - covered 8 edge cases, added mitigations
- Pass 5: Excellence - verified simplicity, added sequence diagram

**Key implementation files:**
1. `td/src/ipc/mod.rs` - Module root, `get_socket_path()`
2. `td/src/ipc/messages.rs` - `DaemonMessage`, `DaemonResponse`
3. `td/src/ipc/client.rs` - `DaemonClient`
4. `td/src/loop/manager.rs` - Add IPC handling to `run()` select loop
5. `td/src/state/manager.rs` - Add IPC notification to state change methods
6. `td/src/main.rs` - Create and bind socket in `run_daemon()`
