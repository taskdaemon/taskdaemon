# Design Document: TaskStore - Generic Persistent Storage Library

**Author:** Claude Sonnet 4.5
**Date:** 2026-01-14
**Status:** Complete
**Review Passes:** 5/5

## Summary

TaskStore is a Rust library providing durable, git-integrated persistent storage for any data type implementing the `Record` trait. It implements the SQLite+JSONL+Git pattern: SQLite for fast queries, JSONL files for git-based truth and merge conflict resolution, and a custom git merge driver for automated conflict handling.

## Problem Statement

### Background

Modern development workflows increasingly rely on git for version control, but many applications need persistent storage that:

1. **Survives crashes:** Application state must persist across restarts and failures
2. **Merges cleanly:** When developers work in separate git branches/worktrees, state should merge without conflicts
3. **Queries efficiently:** Applications need fast indexed lookups, not O(n) scans
4. **Audits transparently:** Teams need human-readable state files for review and debugging
5. **Versions naturally:** State should evolve alongside code in git history

Existing solutions fall short:
- **In-memory only:** Lost on crash, no persistence
- **SQLite alone:** Binary format causes merge conflicts, not git-friendly
- **JSONL alone:** Slow queries, no indexing, O(n) scans
- **Beads:** Overcomplicated, invasive auto-push behavior, poor UX
- **Engram:** Lost git integration (merge driver, hooks) during refactoring

### Problem

**How do we build a generic persistent storage library that is:**
- **Fast:** Sub-millisecond queries for indexed lookups
- **Durable:** Survives crashes, supports graceful shutdown/restart
- **Git-native:** Merges cleanly, works with worktrees and branches
- **Concurrent:** Multiple readers without blocking
- **Auditable:** Human-readable state files for review
- **Simple:** Thin library, minimal API surface
- **Generic:** Works with any data type implementing a trait

### Goals

- Implement SQLite+JSONL+Git pattern with custom merge driver
- Provide generic CRUD operations for any `Record` type
- Support atomic transactions for multi-record updates
- Enable fast queries via SQLite indexes
- Store state as line-delimited JSON (one record per line) for merge-friendliness
- Include git hooks for automatic sync on commit/merge/rebase
- Support graceful degradation: If SQLite is stale, rebuild from JSONL
- Library crate with thin CLI for manual inspection

### Non-Goals

- **Not a distributed database:** Single machine, single process writes
- **Not a replacement for git:** Git remains source of truth for versioning
- **Not a query engine:** No complex SQL beyond simple indexed lookups
- **Not a workflow engine:** Applications handle their own orchestration
- **Not user-facing:** Infrastructure library consumed by applications
- **Not a background service:** Embedded library, not daemon process

## Proposed Solution

### Overview

TaskStore is a Rust library (`taskstore` crate) that maintains two synchronized representations of state:

1. **SQLite database** (`.taskstore/taskstore.db`): Fast indexed queries
2. **JSONL files** (`.taskstore/*.jsonl`): Git-friendly line-delimited JSON records

On every write, both are updated atomically. On startup or after git operations (merge, rebase, pull), TaskStore rebuilds SQLite from JSONL if timestamps indicate staleness.

A custom git merge driver resolves conflicts in JSONL files by merging record-by-record using unique IDs.

Git hooks (pre-commit, post-merge, post-rebase) trigger sync operations to keep SQLite current.

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Your Application                       │
│         (plans, users, tasks, events, etc.)              │
└────────────────┬────────────────────────────────────────┘
                 │
                 │ uses
                 ▼
┌─────────────────────────────────────────────────────────┐
│                   TaskStore Library                      │
│  ┌─────────────────────────────────────────────────┐   │
│  │              Public API                          │   │
│  │  - Store::open(path)                            │   │
│  │  - store.create<T>(record)                      │   │
│  │  - store.get<T>(id)                             │   │
│  │  - store.update<T>(id, record)                  │   │
│  │  - store.list<T>(filter)                        │   │
│  │  - store.sync()                                  │   │
│  └─────────────┬───────────────────────────────────┘   │
│                │                                         │
│  ┌─────────────┼───────────────────────────────────┐   │
│  │             ▼                                     │   │
│  │      Internal Implementation                     │   │
│  │                                                   │   │
│  │  ┌──────────────┐      ┌──────────────────┐    │   │
│  │  │   SQLite     │◄────►│    JSONL Files   │    │   │
│  │  │   Database   │      │   (line-per-rec) │    │   │
│  │  │              │      │                   │    │   │
│  │  │  Fast query  │      │  Git-friendly     │    │   │
│  │  │  Indexed     │      │  Human-readable   │    │   │
│  │  └──────────────┘      └──────────────────┘    │   │
│  │                                                   │   │
│  │  Sync Logic: Write → both, Read → SQLite        │   │
│  │  Rebuild: If stale, rebuild SQLite from JSONL   │   │
│  └───────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│                    Git Integration                       │
│                                                          │
│  Custom Merge Driver (.gitattributes)                   │
│    *.jsonl merge=taskstore-merge                        │
│                                                          │
│  Git Hooks:                                             │
│    pre-commit:   sync() before commit                   │
│    post-merge:   sync() after merge                     │
│    post-rebase:  sync() after rebase                    │
└─────────────────────────────────────────────────────────┘
```

### Data Model

#### The Record Trait

All types stored in TaskStore must implement the `Record` trait:

```rust
pub trait Record: Serialize + DeserializeOwned + Clone {
    /// Unique table name for this record type (e.g., "plans", "users")
    fn table_name() -> &'static str;

    /// Get the unique ID for this record
    fn id(&self) -> &str;

    /// Get the last updated timestamp (Unix ms)
    fn updated_at(&self) -> i64;

    /// Define the SQLite schema for this record type
    fn schema() -> &'static str;

    /// Convert record to SQL parameter tuple for INSERT/UPDATE
    fn to_params(&self) -> Vec<Box<dyn rusqlite::ToSql>>;

    /// Construct record from SQL row
    fn from_row(row: &rusqlite::Row) -> Result<Self>;
}
```

#### Example: Plan Record

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: PlanStatus,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Draft,
    Active,
    Complete,
}

impl Record for Plan {
    fn table_name() -> &'static str { "plans" }
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }

    fn schema() -> &'static str {
        r#"
        CREATE TABLE plans (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            status TEXT NOT NULL,
            content TEXT NOT NULL
        );
        CREATE INDEX idx_plans_status ON plans(status);
        "#
    }

    fn to_params(&self) -> Vec<Box<dyn rusqlite::ToSql>> {
        vec![
            Box::new(self.id.clone()),
            Box::new(self.title.clone()),
            Box::new(self.description.clone()),
            Box::new(self.created_at),
            Box::new(self.updated_at),
            Box::new(format!("{:?}", self.status).to_lowercase()),
            Box::new(self.content.clone()),
        ]
    }

    fn from_row(row: &rusqlite::Row) -> Result<Self> {
        Ok(Plan {
            id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            status: match row.get::<_, String>(5)?.as_str() {
                "draft" => PlanStatus::Draft,
                "active" => PlanStatus::Active,
                "complete" => PlanStatus::Complete,
                _ => return Err(eyre!("Invalid status")),
            },
            content: row.get(6)?,
        })
    }
}
```

#### Example: User Record

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub active: bool,
}

impl Record for User {
    fn table_name() -> &'static str { "users" }
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }

    fn schema() -> &'static str {
        r#"
        CREATE TABLE users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            email TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            active INTEGER NOT NULL
        );
        CREATE INDEX idx_users_username ON users(username);
        CREATE INDEX idx_users_active ON users(active);
        "#
    }

    fn to_params(&self) -> Vec<Box<dyn rusqlite::ToSql>> {
        vec![
            Box::new(self.id.clone()),
            Box::new(self.username.clone()),
            Box::new(self.email.clone()),
            Box::new(self.created_at),
            Box::new(self.updated_at),
            Box::new(self.active as i64),
        ]
    }

    fn from_row(row: &rusqlite::Row) -> Result<Self> {
        Ok(User {
            id: row.get(0)?,
            username: row.get(1)?,
            email: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            active: row.get::<_, i64>(5)? != 0,
        })
    }
}
```

#### Directory Structure

```
.taskstore/
├── taskstore.db           # SQLite database (fast queries)
├── plans.jsonl            # Plan records (one per line)
├── users.jsonl            # User records
├── tasks.jsonl            # Task records
├── events.jsonl           # Event records
├── taskstore.log          # Structured log output (rotated)
└── .version               # Schema version marker
```

#### Generic SQLite Schema

The library maintains a generic registry of tables:

```sql
-- Generic record storage
CREATE TABLE records (
    table_name TEXT NOT NULL,      -- e.g., "plans", "users"
    id TEXT NOT NULL,               -- Record ID
    data TEXT NOT NULL,             -- Full JSON record
    updated_at INTEGER NOT NULL,    -- Unix timestamp (ms)
    PRIMARY KEY (table_name, id)
);

-- Index for common queries
CREATE INDEX idx_records_table ON records(table_name);
CREATE INDEX idx_records_updated ON records(updated_at);

-- Type-specific tables are created dynamically via Record::schema()
```

When you register a type, its specific table is created using the schema from `Record::schema()`.

#### JSONL Format

Each JSONL file contains one JSON object per line. Each record has a unique `id` field.

**plans.jsonl example:**
```jsonl
{"id":"550e8400-e29b-41d4-a716-446655440000","title":"Launch Product","description":"Complete product launch...","created_at":1704067200000,"updated_at":1704067200000,"status":"active","content":"# Plan: Launch Product\n\n..."}
{"id":"660e8400-e29b-41d4-a716-446655440001","title":"Migrate Database","description":"Upgrade to PostgreSQL 15...","created_at":1704070800000,"updated_at":1704070800000,"status":"draft","content":"# Plan: Migrate Database\n\n..."}
```

**users.jsonl example:**
```jsonl
{"id":"770e8400-e29b-41d4-a716-446655440010","username":"alice","email":"alice@example.com","created_at":1704074400000,"updated_at":1704078000000,"active":true}
{"id":"880e8400-e29b-41d4-a716-446655440020","username":"bob","email":"bob@example.com","created_at":1704074500000,"updated_at":1704078100000,"active":true}
```

### API Design

```rust
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// Main store handle
pub struct Store {
    db: rusqlite::Connection,
    base_path: PathBuf,
}

impl Store {
    /// Open or create store at given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self>;

    /// Register a record type (creates table if needed)
    pub fn register<T: Record>(&mut self) -> Result<()>;

    /// Sync: Rebuild SQLite from JSONL if needed
    pub fn sync(&mut self) -> Result<()>;

    /// Flush: Ensure all writes are persisted
    pub fn flush(&mut self) -> Result<()>;
}

// Generic CRUD operations
impl Store {
    /// Create a new record
    pub fn create<T: Record>(&mut self, record: T) -> Result<String> {
        // Returns record ID
    }

    /// Get record by ID
    pub fn get<T: Record>(&self, id: &str) -> Result<Option<T>>;

    /// Update existing record
    pub fn update<T: Record>(&mut self, id: &str, record: T) -> Result<()>;

    /// Delete record by ID
    pub fn delete<T: Record>(&mut self, id: &str) -> Result<()>;

    /// List all records of type T
    pub fn list<T: Record>(&self) -> Result<Vec<T>>;

    /// List records matching a filter
    pub fn list_filtered<T: Record>(&self, filter: Filter) -> Result<Vec<T>>;

    /// List records with pagination
    pub fn list_paginated<T: Record>(
        &self,
        filter: Option<Filter>,
        limit: usize,
        offset: usize
    ) -> Result<Vec<T>>;

    /// Count records matching filter
    pub fn count<T: Record>(&self, filter: Option<Filter>) -> Result<usize>;
}

// Filter builder for queries
#[derive(Debug, Clone)]
pub struct Filter {
    conditions: Vec<(String, FilterOp, Box<dyn rusqlite::ToSql>)>,
}

#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq,      // =
    Ne,      // !=
    Lt,      // <
    Lte,     // <=
    Gt,      // >
    Gte,     // >=
    Like,    // LIKE
}

impl Filter {
    pub fn new() -> Self;
    pub fn eq<T: rusqlite::ToSql>(mut self, field: &str, value: T) -> Self;
    pub fn ne<T: rusqlite::ToSql>(mut self, field: &str, value: T) -> Self;
    pub fn gt<T: rusqlite::ToSql>(mut self, field: &str, value: T) -> Self;
    // ... other filter methods
}
```

### Usage Examples

**Creating records:**
```rust
let mut store = Store::open(".taskstore")?;

// Register record types
store.register::<Plan>()?;
store.register::<User>()?;

// Create a plan
let plan = Plan {
    id: uuid::Uuid::now_v7().to_string(),
    title: "Launch Product".to_string(),
    description: "Complete product launch".to_string(),
    created_at: now_ms(),
    updated_at: now_ms(),
    status: PlanStatus::Draft,
    content: "# Plan: Launch Product\n\n...".to_string(),
};

let plan_id = store.create(plan)?;
println!("Created plan: {}", plan_id);

// Create a user
let user = User {
    id: uuid::Uuid::now_v7().to_string(),
    username: "alice".to_string(),
    email: "alice@example.com".to_string(),
    created_at: now_ms(),
    updated_at: now_ms(),
    active: true,
};

let user_id = store.create(user)?;
println!("Created user: {}", user_id);
```

**Querying records:**
```rust
// Get all active plans
let active_plans = store.list_filtered::<Plan>(
    Filter::new().eq("status", "active")
)?;

for plan in active_plans {
    println!("Plan {}: {}", plan.id, plan.title);
}

// Get all active users
let active_users = store.list_filtered::<User>(
    Filter::new().eq("active", 1)
)?;

for user in active_users {
    println!("User {}: {}", user.id, user.username);
}

// Get single record by ID
if let Some(plan) = store.get::<Plan>("550e8400-...")? {
    println!("Found plan: {}", plan.title);
}
```

**Pagination:**
```rust
// Get first page (100 records)
let page1 = store.list_paginated::<Plan>(None, 100, 0)?;

// Get second page (next 100 records)
let page2 = store.list_paginated::<Plan>(None, 100, 100)?;
```

**Updating records:**
```rust
if let Some(mut plan) = store.get::<Plan>("550e8400-...")? {
    plan.status = PlanStatus::Active;
    plan.updated_at = now_ms();
    store.update(&plan.id, plan)?;
}
```

### Merge Driver Algorithm

The `taskstore-merge` binary implements a three-way merge for JSONL files:

1. **Parse all three versions:** base (common ancestor), ours (current branch), theirs (incoming branch)
2. **Build ID maps:** Index all records by their unique `id` field
3. **Merge logic per record:**
   - If only in ours: Keep ours
   - If only in theirs: Add theirs
   - If in both: Compare `updated_at` timestamps, keep newer version
   - If same timestamp: Mark as conflict, write both with `<<<<<<< OURS` and `>>>>>>> THEIRS` markers (user must resolve manually)
4. **Write merged result:** One record per line, sorted by ID for determinism

**Example:**
```
Base:    {"id":"A","updated_at":1000,"status":"draft"}
Ours:    {"id":"A","updated_at":1001,"status":"active"}
Theirs:  {"id":"A","updated_at":1002,"status":"complete"}
Merged:  {"id":"A","updated_at":1002,"status":"complete"}  # Theirs wins (newer)
```

**Conflict example (same timestamp):**
```
<<<<<<< OURS
{"id":"B","updated_at":1500,"status":"active"}
=======
{"id":"B","updated_at":1500,"status":"draft"}
>>>>>>> THEIRS
```

User must manually edit to resolve.

### Staleness Detection Algorithm

SQLite is considered stale if any JSONL file is newer than the database file:

```rust
fn is_stale(db_path: &Path, jsonl_files: &[PathBuf]) -> Result<bool> {
    let db_mtime = fs::metadata(db_path)?.modified()?;
    for jsonl in jsonl_files {
        let jsonl_mtime = fs::metadata(jsonl)?.modified()?;
        if jsonl_mtime > db_mtime {
            return Ok(true);
        }
    }
    Ok(false)
}
```

On startup and after git hooks, check staleness. If stale, call `sync()` to rebuild.

### JSONL Update Semantics

JSONL files are **append-only logs**. Updates work as follows:

1. **Write operation:** Append new record to JSONL (even if updating existing ID)
2. **Result:** Multiple records with same ID, differing `updated_at` timestamps
3. **Sync operation:** When rebuilding SQLite from JSONL, take the **latest** record per ID (highest `updated_at`)

**Example plans.jsonl:**
```jsonl
{"id":"A","title":"Feature X","status":"draft","updated_at":1000}
{"id":"A","title":"Feature X","status":"active","updated_at":1001}
{"id":"A","title":"Feature X","status":"complete","updated_at":1002}
```

When syncing, only the last record (complete, 1002) is inserted into SQLite.

**Benefits:**
- Merge conflicts are rare (append-only)
- Full audit trail (all versions preserved)
- Sync is idempotent (can rebuild anytime)

**Compaction (future):** Periodically rewrite JSONL files keeping only latest record per ID to reclaim space.

### Transaction Boundaries and Dual-Write Ordering

**Critical invariant:** JSONL is source of truth. SQLite is derived cache.

**Write ordering:** Always write JSONL first, then SQLite.

```rust
pub fn update_multi<T: Record>(&mut self, records: Vec<T>) -> Result<()> {
    // 1. Write to JSONL first (source of truth)
    for record in &records {
        self.append_jsonl(T::table_name(), record)?;
    }

    // 2. Then update SQLite (derived cache)
    let tx = self.db.transaction()?;
    for record in &records {
        tx.execute(
            &format!("UPDATE {} SET ... WHERE id = ?", T::table_name()),
            record.to_params()
        )?;
    }
    tx.commit()?;

    Ok(())
}
```

**Failure scenarios:**
- **JSONL write fails:** Return error immediately, nothing written, consistent state
- **SQLite write fails after JSONL succeeds:** SQLite is now stale, but next `sync()` will rebuild from JSONL and repair

### Error Handling Patterns

**Update non-existent record:** Return `Err(eyre!("Record not found: {}", id))`

**Create duplicate ID:** Check SQLite first, return `Err(eyre!("Record already exists: {}", id))` if found

**JSONL write failure:** Return error immediately (may indicate disk full, permission issues, or read-only filesystem)

**SQLite corruption:** Rebuild from JSONL via `sync()`

**Malformed JSONL line:** Skip line, emit `tracing::warn!()`, continue sync (graceful degradation)

**Empty JSONL file:** Treat as "no records", sync results in empty table (valid state)

**Deleted JSONL file:** Treat as "no records" if file doesn't exist, sync creates empty table

**JSONL line exceeds size limit:** Reject writes >10MB per record, return error (prevents unbounded memory usage)

**File permission denied:** Return error with context about .taskstore/ permissions

**Concurrent access:** Document as unsupported - single-process only, external writes are undefined behavior

### Logging Strategy

Use `tracing` crate for structured logging:

```rust
use tracing::{info, warn, error};

// Info: Normal operations
info!(store_path = ?self.base_path, "Opening TaskStore");

// Warn: Recoverable issues
warn!(line_no = line_num, file = "plans.jsonl", "Skipping malformed JSONL line");

// Error: Critical failures
error!(error = ?e, "Failed to write JSONL, data loss possible");
```

Applications configure tracing subscriber to write to `.taskstore/taskstore.log` for debugging.

### Edge Case: Timestamps

All timestamps are **Unix milliseconds (UTC)**. No timezone conversion logic in store.

```rust
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before Unix epoch")
        .as_millis() as i64
}
```

### Scale Limits and Pagination

**Expected scale:**
- **Records per type:** 1K-100K typical, up to 1M supported
- **Total records:** Up to 10M across all types
- **JSONL file size:** Up to 1GB per file before compaction recommended
- **Record size:** Up to 10MB per record (enforced limit)

**Pagination support:**

```rust
pub fn list_paginated<T: Record>(
    &self,
    filter: Option<Filter>,
    limit: usize,
    offset: usize
) -> Result<Vec<T>>;
```

Applications use pagination for large result sets. Default limit: 100 records.

### JSONL Compaction Strategy

**Trigger conditions:**
- JSONL file exceeds 100MB
- More than 80% of lines are superseded (older versions of same ID)
- Manual invocation via `taskstore compact` CLI command

**Compaction algorithm:**
1. Read entire JSONL file (e.g., `plans.jsonl`)
2. Build map of ID → latest record (highest updated_at)
3. Write compacted records to temporary file (e.g., `plans.jsonl.tmp`)
4. Fsync temporary file to disk
5. Atomically rename: `rename("plans.jsonl.tmp", "plans.jsonl")`
6. Trigger sync() to rebuild SQLite

**Atomicity:** `rename()` is atomic on POSIX filesystems. If compaction crashes mid-way, original file is preserved.

**Safety:** Compaction preserves all current state, only removes superseded history.

### Observability and Metrics

TaskStore exposes metrics for monitoring:

```rust
pub struct StoreMetrics {
    pub write_latency_ms: f64,       // Exponential moving average of last 100 writes
    pub sync_duration_ms: f64,       // Duration of last sync() call
    pub record_counts: HashMap<String, usize>,  // Count per table
    pub jsonl_sizes_mb: HashMap<String, f64>,   // Size per JSONL file
}

impl Store {
    pub fn metrics(&self) -> Result<StoreMetrics>;
}
```

**Collection timing:**
- Write latency: Measured on each write, EMA updated
- Sync duration: Recorded at end of each sync() call
- Counts: Queried on-demand when metrics() is called
- File sizes: Queried on-demand via fs::metadata()

**Usage:**
- Applications call `store.metrics()` periodically for monitoring
- Optional: Expose via Prometheus endpoint for dashboards

### Backup and Restore Strategy

**Backup:**
- JSONL files are the source of truth
- To backup: `cp -r .taskstore/ backup/taskstore-$(date +%s)/`
- Or commit .taskstore/ to git (recommended)

**Restore:**
- Copy JSONL files to .taskstore/
- Delete taskstore.db (or let it be stale)
- Call `taskstore sync` or `Store::open()` (auto-syncs)
- SQLite rebuilds from JSONL

**Git-based backup (recommended):**
- .gitignore: `taskstore.db` and `taskstore.log`
- Commit JSONL files: `plans.jsonl`, `users.jsonl`, etc.
- Push to remote for off-machine backup

### Git Integration

**Store lifetime:**
- Application creates single Store instance at startup
- Store lives for entire application lifetime
- On shutdown, call `store.flush()` to ensure persistence

**Change notification:**
- TaskStore is passive (no pub/sub, no watchers)
- Applications poll Store periodically or after known writes
- For real-time coordination, use in-memory channels with Store as durable backup

**Multi-repo support:**
- Each repo has its own `.taskstore/` directory at repo root
- Applications can manage multiple repos, opens one Store per repo
- Store path: `<repo_root>/.taskstore/`

### Schema Migration Strategy

The `.version` file contains current schema version as integer (e.g., `1`).

**Migration process:**
1. On `Store::open()`, read `.version` file
2. Compare to `CURRENT_VERSION` constant in code
3. If behind, run migration functions sequentially
4. Update `.version` file after successful migration

**Example migrations:**
```rust
const CURRENT_VERSION: u32 = 2;

fn migrate_1_to_2(store: &mut Store) -> Result<()> {
    // Add new column to existing table
    store.db.execute(
        "ALTER TABLE plans ADD COLUMN priority INTEGER DEFAULT 0",
        []
    )?;
    // Rebuild from JSONL to populate new column with defaults
    store.sync()?;
    Ok(())
}
```

**JSONL schema evolution:** JSON is flexible, missing fields default via serde. For breaking changes, write migration that rewrites JSONL files.

### Implementation Plan

#### Phase 1: Generic Record Trait and Core Structure
- Define `Record` trait with required methods
- Create `taskstore` crate with library + binary structure:
  - `src/lib.rs` - Main library exports (pub use Store, Record, Filter, etc.)
  - `src/store.rs` - Generic Store implementation
  - `src/record.rs` - Record trait definition
  - `src/filter.rs` - Filter builder for queries
  - `src/jsonl.rs` - JSONL read/write operations
  - `src/sqlite.rs` - SQLite operations
  - `src/bin/taskstore.rs` - CLI binary
- Update Cargo.toml with `[lib]` and `[[bin]]` sections
- Implement basic Store::open() with staleness check and sync()
- Add directory structure creation (.taskstore/ with .gitignore for .db file)
- Implement schema version tracking and migration framework

#### Phase 2: Generic CRUD Operations
- Implement `create<T: Record>()` method
- Implement `get<T: Record>()` method
- Implement `update<T: Record>()` method
- Implement `delete<T: Record>()` method
- Implement `list<T: Record>()` method
- Implement `register<T: Record>()` for dynamic table creation
- Add automatic table creation from `Record::schema()`

#### Phase 3: Filter and Query System
- Implement Filter builder with operator support
- Implement `list_filtered<T: Record>()` method
- Implement `list_paginated<T: Record>()` method
- Implement `count<T: Record>()` method
- Add SQLite prepared statements for efficiency
- Add query result caching (optional)

#### Phase 4: JSONL Persistence
- Implement write-through: Every SQL write appends to JSONL
- Implement sync: Rebuild SQLite from JSONL if stale
- Add timestamp-based staleness detection
- Handle JSONL file rotation if files grow too large (future)
- Implement compaction strategy

#### Phase 5: Git Merge Driver
- Write `taskstore-merge` binary that takes 3 args: base, ours, theirs file paths
- Implement line-by-line merge using unique IDs and updated_at timestamps
- Handle conflict resolution: manual markers if timestamps equal
- Exit 0 on success, non-zero on unresolvable conflict

**Git configuration:**
```bash
# In .git/config or ~/.gitconfig
[merge "taskstore-merge"]
    name = TaskStore JSONL merge driver
    driver = taskstore-merge %O %A %B
```

**In .gitattributes:**
```
.taskstore/*.jsonl merge=taskstore-merge
```

**Installation helper:**
```rust
impl Store {
    pub fn install_git_integration(&self) -> Result<()> {
        // Write .gitattributes
        // Configure git merge driver
        // Install git hooks
    }
}
```

#### Phase 6: Git Hooks
- Write hook scripts that shell out to `taskstore sync`
- Install hooks to `.git/hooks/` (make executable)
- Hook content:

**post-merge hook:**
```bash
#!/bin/bash
# Auto-sync TaskStore after merge
cd "$(git rev-parse --show-toplevel)"
taskstore sync || echo "Warning: TaskStore sync failed"
exit 0  # Don't block merge on sync failure
```

Same pattern for post-rebase and pre-commit.

Installation via `store.install_git_integration()` or manual:
```bash
taskstore install-hooks
```

#### Phase 7: CLI Tool
- Add `taskstore` binary for manual inspection
- Commands:
  - `taskstore list <type> [--filter KEY=VALUE] [--limit N] [--offset N]` - List records by type
  - `taskstore show <type> <id>` - Show detailed record by ID and type
  - `taskstore sync` - Force rebuild SQLite from JSONL
  - `taskstore compact [<type>]` - Compact JSONL files (all or specific type)
  - `taskstore metrics` - Display store metrics (record counts, file sizes, latencies)
  - `taskstore backup <dest>` - Copy JSONL files to destination directory
  - `taskstore install-hooks` - Install git hooks and merge driver
  - `taskstore register <type>` - Register a new record type (requires schema)
- Pretty-printed output using colored terminal output (colored crate)

**Example CLI output:**

```
$ taskstore list plans --filter status=active
ID                                      Title                  Status    Updated
────────────────────────────────────────────────────────────────────────────────
01928374-abcd-7000-a123-456789012345   Launch Product         active    2026-01-14
01928375-beef-7000-a456-789012345678   Migrate Database       active    2026-01-14

$ taskstore metrics
TaskStore Metrics
─────────────────────────────────────
Tables:
  plans:         42 records
  users:         87 records
  tasks:         234 records
  events:        1,543 records

Write Latency:     0.8ms (avg)
Sync Duration:     127ms (last)

JSONL File Sizes:
  plans.jsonl:     8.4 MB
  users.jsonl:     2.1 MB
  tasks.jsonl:     15.7 MB
  events.jsonl:    42.3 MB
```

#### Phase 8: Testing & Documentation
- Unit tests for all CRUD operations
- Integration tests for sync behavior
- Test merge driver with conflicting JSONL files
- Write comprehensive README.md with examples
- Add rustdoc comments to all public APIs
- Create example applications demonstrating Record trait implementation

## Alternatives Considered

### Alternative 1: Pure SQLite (No JSONL)
- **Description:** Store everything in SQLite only, commit the .db file to git
- **Pros:** Simpler implementation, no dual-write overhead
- **Cons:** Binary files cause merge conflicts, not human-readable, git diffs useless
- **Why not chosen:** Git integration is critical for auditability and team collaboration

### Alternative 2: Pure JSONL (No SQLite)
- **Description:** Store everything as JSONL, parse on every query
- **Pros:** Git-native, human-readable, simple implementation
- **Cons:** Slow queries (O(n) scans), no indexing, poor performance at scale
- **Why not chosen:** Unacceptable performance for filtered lookups and pagination

### Alternative 3: Use Beads
- **Description:** Adopt Beads as the datastore
- **Pros:** Already exists, proven merge driver, community support
- **Cons:** Slow, invasive (auto-push), overcomplicated API, poor UX, one-per-line beads clutter git
- **Why not chosen:** Need cleaner, simpler, less invasive storage for generic use cases

### Alternative 4: Use Engram
- **Description:** Fork Engram and restore git integration
- **Pros:** Close to our needs, already has SQLite+JSONL pattern
- **Cons:** Lost merge driver and hooks (would need to re-add), codebase unfamiliar, licensing unclear
- **Why not chosen:** Starting fresh is cleaner than fixing someone else's accidental minimalism

### Alternative 5: Temporal-style Event Sourcing
- **Description:** Store all state changes as append-only event log
- **Pros:** Full history, time-travel debugging, audit trail
- **Cons:** Complexity overkill, slower queries, much larger storage footprint
- **Why not chosen:** Most applications don't need full event sourcing; current state is sufficient

### Alternative 6: Domain-Specific Libraries
- **Description:** Build a custom storage library for each application domain (e.g., project-management-specific, user-management-specific)
- **Pros:** Can optimize for specific use cases, simpler API for domain
- **Cons:** Code duplication, maintenance burden, reinventing the wheel
- **Why not chosen:** Generic infrastructure enables reuse across projects, reduces maintenance

## Technical Considerations

### Dependencies

**Rust crates:**
- `rusqlite` - SQLite bindings
- `serde` + `serde_json` - Serialization
- `uuid` - Unique ID generation (UUIDv7)
- `eyre` - Error handling
- `clap` - CLI argument parsing (for binary)
- `tracing` - Structured logging

**External tools:**
- Git (for merge driver and hooks)

### Performance

**Expected characteristics:**
- **Writes:** ~1ms per record (SQLite write + JSONL append)
- **Reads:** <1ms for indexed lookups
- **Sync:** ~100ms for 10,000 records (rebuild SQLite from JSONL)
- **Merge:** ~10ms per conflicting JSONL line

**Optimization strategies:**
- SQLite WAL mode for concurrent reads during writes
- Batch writes: Transaction wrapping multiple records
- Lazy sync: Only rebuild if stale (timestamp check)
- JSONL streaming: Don't load entire file into memory

### Security

**Threat model:**
- TaskStore runs on local machine with trusted operators
- No network exposure, no authentication needed
- File permissions inherit from git repo

**Considerations:**
- **SQL injection:** Use parameterized queries (rusqlite handles this)
- **Path traversal:** Validate all file paths stay within .taskstore/
- **Malicious JSONL:** Validate records during sync, reject malformed JSON

### Testing Strategy

**Unit tests:**
- Test each CRUD operation in isolation
- Test sync logic with stale/fresh SQLite
- Test JSONL append and read
- Test timestamp staleness detection
- Test Record trait implementations

**Integration tests:**
- Create store, write records, close, reopen, verify persistence
- Simulate git merge conflict, test merge driver
- Trigger git hooks, verify sync is called
- Test multiple record types in same store

**Performance tests:**
- Benchmark CRUD operations with 10K, 100K, 1M records
- Measure sync time for various JSONL file sizes
- Profile memory usage during sync

### Rollout Plan

**Phase 1: Library development**
- Build taskstore as standalone crate
- Test in isolation with unit/integration tests

**Phase 2: Example applications**
- Create sample apps using different Record types
- Validate API ergonomics and flexibility

**Phase 3: Git integration testing**
- Document merge driver installation steps
- Provide hook installation helper
- Test with multiple concurrent git worktrees

**Phase 4: Production hardening**
- Add comprehensive error handling
- Implement graceful degradation (e.g., if JSONL corrupt, rebuild from SQLite)
- Add logging for debugging

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Merge driver conflicts not handled correctly | Medium | High | Thorough testing with real merge scenarios, fall back to manual conflict markers if tie |
| JSONL files grow too large (GBs) | Low | Medium | Implement compaction after threshold, keep only recent versions |
| SQLite corruption | Low | High | Always rebuild from JSONL (source of truth), keep SQLite in .gitignore |
| Dual-write inconsistency (JSONL succeeds, SQLite fails) | Low | Medium | SQLite becomes stale, next sync() rebuilds and repairs automatically |
| Hook installation issues on different platforms | Medium | Low | Provide clear installation instructions, test on Linux/Mac/Windows |
| Performance degradation at scale | Medium | Medium | Profile early, add caching layer if needed, implement compaction |
| Concurrent writes from multiple processes | Low | High | Document: Single-process only, external writes are undefined behavior |
| Concurrent git operations during write | Medium | Medium | Document: Avoid git operations during writes, or use store.flush() first |
| UUIDv7 collision | Very Low | High | Check for duplicates before insert, return error if collision detected |
| Record size exceeds memory limits | Low | Medium | Enforce 10MB per-record limit, reject writes that exceed threshold |
| Type confusion (reading wrong type from table) | Low | High | Store type name in registry, validate on read operations |

## Open Questions

- [ ] Should we archive old records automatically based on age?
- [ ] How large can JSONL files grow before we need rotation?
- [ ] Should TaskStore support multi-process access (e.g., file locking)?
- [ ] Do we need a vacuum/cleanup operation for SQLite?
- [ ] Should we support JSON schema validation for JSONL records?
- [ ] Should we provide built-in relationships between record types (foreign keys)?

## References

- Engram architecture: `~/.config/pais/research/tech/engram-vs-beads/2026-01-12-comparison.md`
- Accidental minimalism lesson: `~/.config/pais/research/tech/engram-vs-beads/2026-01-12-accidental-minimalism.md`
- SQLite WAL mode: https://www.sqlite.org/wal.html
- JSONL format: https://jsonlines.org/
- UUIDv7 specification: https://www.ietf.org/archive/id/draft-peabody-dispatch-new-uuid-format-04.html

## Rule of Five Review

### Review Pass 1: Core Design Clarity
**Question:** Is the generic Record trait design clear and flexible enough for diverse use cases?

**Assessment:** Yes. The Record trait provides a clean abstraction:
- `table_name()` enables multi-type storage
- `id()` and `updated_at()` support merge logic
- `schema()` allows custom table definitions
- `to_params()` and `from_row()` handle SQLite mapping

The trait is minimal yet sufficient for CRUD + merging.

**Concerns:** Type safety between JSONL and SQLite could be stronger. Consider adding runtime validation.

### Review Pass 2: API Ergonomics
**Question:** Are the generic APIs intuitive for library consumers?

**Assessment:** Good. The `create<T>()`, `get<T>()`, `list<T>()` pattern is familiar to Rust developers. Filter builder provides flexible querying without SQL injection risks.

**Improvement:** Add example implementations for common patterns (enums, nested structs, optional fields).

**Concern:** Error messages for schema mismatches need to be clear and actionable.

### Review Pass 3: Git Integration Robustness
**Question:** Will the merge driver handle real-world git workflows reliably?

**Assessment:** Design is sound:
- Timestamp-based conflict resolution handles most cases
- Manual markers for ties preserve data integrity
- Append-only JSONL reduces conflict frequency

**Risk:** Complex multi-person workflows need thorough testing. Document best practices (e.g., always pull before starting work).

### Review Pass 4: Performance at Scale
**Question:** Will the dual-write pattern and sync operations scale to production workloads?

**Assessment:** Reasonable for expected scales:
- 1ms writes acceptable for most applications
- 100ms sync for 10K records is manageable
- JSONL compaction keeps files under control

**Optimization opportunity:** Add write batching API for bulk inserts (avoid N separate JSONL appends).

**Concern:** Large records (multi-MB) could slow sync. Document recommended record size limits.

### Review Pass 5: Documentation and Examples
**Question:** Is there sufficient documentation for developers to adopt this library?

**Assessment:** Design doc is comprehensive, but needs:
- Step-by-step tutorial for implementing Record trait
- Example repo with Plan/User/Task/Event types
- Migration guide for applications using ad-hoc storage
- Troubleshooting guide for common issues (merge conflicts, sync failures)

**Action items:**
- Create examples/ directory with sample Record implementations
- Write CONTRIBUTING.md for adding new features
- Add FAQ section to README
