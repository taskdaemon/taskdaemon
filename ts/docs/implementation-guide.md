# TaskStore Implementation Guide

**Date:** 2026-01-14
**Status:** Active

## Overview

This guide provides practical implementation details for building applications with TaskStore, a generic git-backed storage library. TaskStore provides append-only JSONL persistence with SQLite indexing, automatic git merge resolution, and hooks integration.

**Key concept:** TaskStore is infrastructure. You define your domain types (Plan, Task, User, Event, etc.), implement the `Record` trait, and TaskStore handles storage, querying, git integration, and conflict resolution.

## 1. Repository Structure: Library + Binary

TaskStore is both a library (for use by your application) and a CLI binary (for manual inspection and git hooks).

### Current State (from scaffold)

```
taskstore/
├── Cargo.toml
├── src/
│   ├── main.rs    # Binary only
│   ├── cli.rs
│   └── config.rs
```

### Target State (library + thin CLI)

```
taskstore/
├── Cargo.toml              # [[bin]] section added
├── src/
│   ├── lib.rs              # NEW: Main library (pub use)
│   ├── store.rs            # Generic Store<T: Record> implementation
│   ├── record.rs           # Record trait definition
│   ├── jsonl.rs            # JSONL persistence
│   ├── sqlite.rs           # SQLite operations
│   ├── merge.rs            # Git merge driver
│   ├── filter.rs           # Generic filter types
│   ├── cli.rs              # CLI argument parsing (keep)
│   ├── config.rs           # Config loading (keep)
│   └── bin/
│       └── taskstore.rs    # NEW: Thin CLI (calls lib)
```

### Conversion Steps

#### 1. Create src/lib.rs

```rust
// taskstore/src/lib.rs
pub mod store;
pub mod record;
pub mod jsonl;
pub mod sqlite;
pub mod merge;
pub mod filter;

pub use store::Store;
pub use record::{Record, RecordType};
pub use filter::{Filter, FilterOp};
```

#### 2. Define the Record Trait

```rust
// taskstore/src/record.rs

use serde::{Deserialize, Serialize};

/// Core trait that all stored types must implement.
///
/// Your domain types (Plan, Task, User, etc.) implement this trait
/// to enable generic storage operations.
pub trait Record: Serialize + for<'de> Deserialize<'de> + Clone {
    /// Record type name for routing (e.g., "plans", "tasks", "users")
    fn record_type() -> RecordType where Self: Sized;

    /// Unique identifier for this record
    fn id(&self) -> &str;

    /// Timestamp of last update (for conflict resolution)
    fn updated_at(&self) -> i64;

    /// Set the updated_at timestamp (called during merge)
    fn set_updated_at(&mut self, timestamp: i64);
}

/// Record type descriptor
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecordType {
    /// Type name (e.g., "plans", "tasks")
    pub name: String,
    /// JSONL filename (e.g., "plans.jsonl")
    pub jsonl_file: String,
    /// SQLite table name (e.g., "plans")
    pub table_name: String,
}

impl RecordType {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            jsonl_file: format!("{}.jsonl", name),
            table_name: name.clone(),
            name,
        }
    }
}
```

#### 3. Example Domain Implementation

```rust
// In your application code (not in taskstore)

use serde::{Deserialize, Serialize};
use taskstore::{Record, RecordType};

/// A plan in your project management system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: PlanStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Draft,
    Active,
    Complete,
    Cancelled,
}

impl Record for Plan {
    fn record_type() -> RecordType {
        RecordType::new("plans")
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn set_updated_at(&mut self, timestamp: i64) {
        self.updated_at = timestamp;
    }
}

/// A task in your project management system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub plan_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub assignee: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Complete,
    Blocked,
}

impl Record for Task {
    fn record_type() -> RecordType {
        RecordType::new("tasks")
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn set_updated_at(&mut self, timestamp: i64) {
        self.updated_at = timestamp;
    }
}
```

#### 4. Generic Store API

```rust
// taskstore/src/store.rs

use crate::record::Record;
use crate::filter::Filter;
use eyre::Result;
use std::path::Path;

pub struct Store {
    base_path: PathBuf,
    db: Connection,
}

impl Store {
    /// Open or create a store at the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let base_path = path.as_ref().to_path_buf();
        fs::create_dir_all(&base_path)?;

        let db_path = base_path.join("store.db");
        let db = Connection::open(db_path)?;

        // Initialize generic schema
        Self::init_schema(&db)?;

        Ok(Self { base_path, db })
    }

    /// Create a new record
    pub fn create<T: Record>(&mut self, record: T) -> Result<String> {
        let record_type = T::record_type();

        // 1. Append to JSONL (source of truth)
        self.append_jsonl(&record_type.jsonl_file, &record)?;

        // 2. Insert into SQLite (derived index)
        self.insert_record(&record)?;

        Ok(record.id().to_string())
    }

    /// Get a record by ID
    pub fn get<T: Record>(&self, id: &str) -> Result<Option<T>> {
        let record_type = T::record_type();

        let mut stmt = self.db.prepare(
            "SELECT data FROM records WHERE type = ?1 AND id = ?2"
        )?;

        let result = stmt.query_row(params![record_type.name, id], |row| {
            let json: String = row.get(0)?;
            Ok(serde_json::from_str(&json).unwrap())
        });

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List records with optional filtering
    pub fn list<T: Record>(&self, filter: Option<Filter>) -> Result<Vec<T>> {
        let record_type = T::record_type();

        let (where_clause, params) = if let Some(f) = filter {
            f.to_sql()
        } else {
            (String::new(), vec![])
        };

        let query = format!(
            "SELECT data FROM records WHERE type = ?1 {}",
            where_clause
        );

        let mut stmt = self.db.prepare(&query)?;
        let mut all_params = vec![record_type.name.clone().into()];
        all_params.extend(params);

        let records = stmt.query_map(&all_params[..], |row| {
            let json: String = row.get(0)?;
            Ok(serde_json::from_str(&json).unwrap())
        })?;

        Ok(records.collect::<Result<Vec<_>, _>>()?)
    }

    /// Update an existing record
    pub fn update<T: Record>(&mut self, record: T) -> Result<()> {
        let record_type = T::record_type();

        // 1. Append to JSONL (source of truth)
        self.append_jsonl(&record_type.jsonl_file, &record)?;

        // 2. Update SQLite (derived index)
        self.update_record(&record)?;

        Ok(())
    }

    /// Rebuild SQLite from JSONL (used by git hooks)
    pub fn sync(&mut self) -> Result<()> {
        // Clear all records
        self.db.execute("DELETE FROM records", [])?;
        self.db.execute("DELETE FROM record_indexes", [])?;

        // Re-import from all JSONL files
        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                self.import_jsonl(&path)?;
            }
        }

        Ok(())
    }
}
```

#### 5. Update Cargo.toml

```toml
[package]
name = "taskstore"
version = "0.1.0"
edition = "2024"

[lib]
name = "taskstore"
path = "src/lib.rs"

[[bin]]
name = "taskstore"
path = "src/bin/taskstore.rs"

[[bin]]
name = "taskstore-merge"
path = "src/bin/taskstore-merge.rs"

[dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
eyre = "0.6"
uuid = { version = "1.0", features = ["v7"] }
clap = { version = "4.0", features = ["derive"] }
```

#### 6. Example CLI Usage

```rust
// src/bin/taskstore.rs - Generic CLI

use clap::Parser;
use taskstore::Store;
use eyre::Result;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Parser)]
enum Command {
    /// List records of a given type
    List {
        /// Record type (e.g., "plans", "tasks")
        record_type: String,
        /// Filter expression (optional)
        #[arg(long)]
        filter: Option<String>,
    },
    /// Show a record by ID
    Show {
        /// Record ID
        id: String,
    },
    /// Rebuild SQLite from JSONL
    Sync,
    /// Install git hooks and merge driver
    InstallHooks,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::open(".taskstore")?;

    match cli.command {
        Command::List { record_type, filter } => {
            // Generic list - prints raw JSON
            let data = store.list_raw(&record_type, filter.as_deref())?;
            for json in data {
                println!("{}", json);
            }
        }
        Command::Show { id } => {
            let data = store.get_raw(&id)?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        Command::Sync => {
            store.sync()?;
            println!("TaskStore synced successfully");
        }
        Command::InstallHooks => {
            store.install_git_integration()?;
            println!("Git hooks and merge driver installed");
        }
    }

    Ok(())
}
```

## 2. File Naming Conventions

### Rust Module Names

**Rule:** Use short, single-word module names to avoid underscores entirely.

```
taskstore/src/
├── lib.rs
├── store.rs      # mod store; (not store_manager)
├── record.rs     # mod record;
├── jsonl.rs      # mod jsonl;
├── sqlite.rs     # mod sqlite;
├── merge.rs      # mod merge; (git merge driver)
├── filter.rs     # mod filter;
└── bin/
    ├── taskstore.rs
    └── taskstore-merge.rs
```

### JSONL Field Names

**Rule:** Use snake_case in JSONL (matches Rust struct fields directly)

```jsonl
{"id":"plan-550e8400","title":"Launch MVP","status":"active","created_at":1704067200000,"updated_at":1704067200000}
```

**Rust struct:**
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    pub created_at: i64,
    pub updated_at: i64,
}
```

**Note:** No serde rename needed - JSONL uses snake_case, Rust uses snake_case.

### Markdown File Names (Optional)

If your records have associated markdown files:

**Rule:** Lowercase with hyphens, sanitize special characters

```rust
fn sanitize_filename(title: &str) -> String {
    title
        .to_lowercase()
        .replace(char::is_whitespace, "-")
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "")
}

// "Launch MVP" → "launch-mvp.md"
```

## 3. Database Schema

### Generic Schema Design

TaskStore uses a **generic schema** that works for all record types:

```sql
-- Core records table (stores all record types)
CREATE TABLE records (
    id TEXT NOT NULL,
    type TEXT NOT NULL,              -- Record type name ("plans", "tasks", etc.)
    data TEXT NOT NULL,              -- Full JSON serialization
    updated_at INTEGER NOT NULL,     -- For conflict resolution
    PRIMARY KEY (type, id)
);

-- Generic indexes for common queries
CREATE TABLE record_indexes (
    record_type TEXT NOT NULL,
    record_id TEXT NOT NULL,
    index_name TEXT NOT NULL,        -- Field name being indexed
    index_value TEXT NOT NULL,       -- Field value (as string)
    FOREIGN KEY (record_type, record_id) REFERENCES records(type, id) ON DELETE CASCADE,
    PRIMARY KEY (record_type, index_name, index_value, record_id)
);

-- Performance indexes
CREATE INDEX idx_records_type ON records(type);
CREATE INDEX idx_records_updated_at ON records(updated_at);
CREATE INDEX idx_record_indexes_lookup ON record_indexes(record_type, index_name, index_value);
```

### How It Works

**All record types share the same schema:**

1. **records table:** Stores full JSON for each record
2. **record_indexes table:** Stores extracted fields for fast filtering

**Example:** Storing a Plan record:

```rust
let plan = Plan {
    id: "plan-001".to_string(),
    title: "Launch MVP".to_string(),
    status: PlanStatus::Active,
    created_at: 1704067200000,
    updated_at: 1704067200000,
};

// Stored as:
// records table:
//   type="plans", id="plan-001", data='{"id":"plan-001","title":"Launch MVP",...}'
//
// record_indexes table:
//   (plans, plan-001, "status", "active")
//   (plans, plan-001, "title", "Launch MVP")
```

### Registering Indexes

Tell TaskStore which fields to index:

```rust
impl Store {
    /// Register indexed fields for a record type
    pub fn register_indexes<T: Record>(&mut self) -> Result<()> {
        let record_type = T::record_type();

        // Define which fields to index
        // This is application-specific
        match record_type.name.as_str() {
            "plans" => {
                self.add_index(&record_type, "status")?;
                self.add_index(&record_type, "title")?;
            }
            "tasks" => {
                self.add_index(&record_type, "plan_id")?;
                self.add_index(&record_type, "status")?;
                self.add_index(&record_type, "assignee")?;
            }
            _ => {}
        }

        Ok(())
    }
}
```

Or use a trait-based approach:

```rust
pub trait Indexable: Record {
    /// Return list of field names to index
    fn indexed_fields() -> Vec<&'static str>;

    /// Extract field value for indexing
    fn field_value(&self, field: &str) -> Option<String>;
}

impl Indexable for Plan {
    fn indexed_fields() -> Vec<&'static str> {
        vec!["status", "title"]
    }

    fn field_value(&self, field: &str) -> Option<String> {
        match field {
            "status" => Some(format!("{:?}", self.status).to_lowercase()),
            "title" => Some(self.title.clone()),
            _ => None,
        }
    }
}
```

### Schema Migrations

Generic schema rarely needs migrations. If you do need to add system fields:

```rust
fn migrate_v1_to_v2(conn: &Connection) -> Result<()> {
    conn.execute(
        "ALTER TABLE records ADD COLUMN archived INTEGER DEFAULT 0",
        [],
    )?;
    Ok(())
}

const CURRENT_VERSION: u32 = 2;

pub fn migrate(store_path: &Path) -> Result<()> {
    let version_file = store_path.join(".version");
    let current = read_version(&version_file)?;

    if current < 2 {
        migrate_v1_to_v2(&conn)?;
        write_version(&version_file, 2)?;
    }

    Ok(())
}
```

## 4. JSONL Patterns

### Append-Only Writes

**Every update appends a new line:**

```rust
pub fn update<T: Record>(&mut self, record: T) -> Result<()> {
    let record_type = T::record_type();

    // 1. Append to JSONL (source of truth)
    let json = serde_json::to_string(&record)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(self.base_path.join(&record_type.jsonl_file))?;
    writeln!(file, "{}", json)?;
    file.sync_all()?;  // fsync

    // 2. Update SQLite (derived cache)
    self.db.execute(
        "UPDATE records SET data = ?1, updated_at = ?2 WHERE type = ?3 AND id = ?4",
        params![json, record.updated_at(), record_type.name, record.id()],
    )?;

    Ok(())
}
```

**Result:** Multiple lines with same ID in JSONL:
```jsonl
{"id":"task-001","status":"pending","updated_at":1000}
{"id":"task-001","status":"in_progress","updated_at":1001}
{"id":"task-001","status":"complete","updated_at":1002}
```

### Sync: JSONL → SQLite

```rust
pub fn sync(&mut self) -> Result<()> {
    // 1. Read all JSONL files
    for entry in fs::read_dir(&self.base_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        // 2. Parse and deduplicate
        let records = self.read_jsonl_generic(&path)?;
        let latest = self.deduplicate_by_id(records);

        // 3. Insert into SQLite
        for (id, (type_name, json, updated_at)) in latest {
            self.db.execute(
                "INSERT OR REPLACE INTO records (id, type, data, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, type_name, json, updated_at],
            )?;

            // Rebuild indexes
            self.rebuild_indexes_for_record(&id, &type_name, &json)?;
        }
    }

    Ok(())
}

fn deduplicate_by_id(&self, records: Vec<(String, String, i64)>) -> HashMap<String, (String, String, i64)> {
    let mut latest: HashMap<String, (String, String, i64)> = HashMap::new();

    for (id, type_name, json, updated_at) in records {
        match latest.get(&id) {
            Some((_, _, existing_ts)) if *existing_ts > updated_at => continue,
            _ => {
                latest.insert(id, (type_name, json, updated_at));
            }
        }
    }

    latest
}
```

### Compaction (Optional)

Remove superseded records to reclaim space:

```bash
taskstore compact
```

```rust
pub fn compact(&mut self, filename: &str) -> Result<()> {
    // 1. Read and deduplicate
    let records = self.read_jsonl_generic(filename)?;
    let latest = self.deduplicate_by_id(records);

    // 2. Write to temp file
    let temp = format!("{}.tmp", filename);
    let mut file = File::create(&temp)?;
    for (_, (_, json, _)) in latest.values() {
        writeln!(file, "{}", json)?;
    }
    file.sync_all()?;

    // 3. Atomic rename
    fs::rename(temp, filename)?;

    Ok(())
}
```

## 5. Git Integration (Layer 2)

### Overview

Git integration is CRITICAL for TaskStore. This is Layer 2 of the architecture:

```
Layer 1: Core storage (CRUD, filtering, SQLite + JSONL)
    ↓
Layer 2: Git integration (merge driver, hooks, debouncing) ← THIS LAYER
    ↓
Layer 3: Application logic (your business rules) ← YOUR APPLICATION provides this
```

**Why Layer 2 is critical:**
- Without custom merge driver: Concurrent record creation = merge conflicts requiring manual resolution
- Without git hooks: Database-JSONL inconsistencies, manual sync commands needed
- Without debouncing: Poor performance (100 creates = 100 JSONL writes)

### 5.1. Custom Merge Driver (CRITICAL)

**What it does:** Automatically resolves JSONL conflicts using field-level three-way merging.

**Why it's needed:**
```
Scenario: Two developers create records simultaneously

Without merge driver:
  git merge → CONFLICT (line-based merge fails) → Manual resolution required

With merge driver:
  git merge → Automatic resolution (by ID, latest wins) → No conflict
```

**Implementation:**

```rust
// src/merge.rs

use crate::record::Record;
use eyre::{Context, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Merge JSONL files using three-way merge strategy.
///
/// Algorithm:
/// 1. Parse ancestor, ours, theirs into records
/// 2. Build ID maps (last occurrence wins per file)
/// 3. For each ID present in ours or theirs:
///    - Both modified: Pick latest by updated_at
///    - Only ours: Keep ours
///    - Only theirs: Keep theirs
/// 4. Serialize merged records back to JSONL
pub fn merge_jsonl_files(
    ancestor_path: &Path,
    ours_path: &Path,
    theirs_path: &Path,
) -> Result<String> {
    // Generic merge using JSON values
    merge_generic(ancestor_path, ours_path, theirs_path)
}

/// Generic merge for any record type (uses JSON values)
fn merge_generic(ancestor: &Path, ours: &Path, theirs: &Path) -> Result<String> {
    // 1. Parse all three files
    let ancestor_records = parse_jsonl_generic(ancestor)?;
    let ours_records = parse_jsonl_generic(ours)?;
    let theirs_records = parse_jsonl_generic(theirs)?;

    // 2. Build ID maps (last occurrence wins - handles append-only JSONL)
    let ancestor_map = build_latest_map(ancestor_records);
    let ours_map = build_latest_map(ours_records);
    let theirs_map = build_latest_map(theirs_records);

    // 3. Three-way merge
    let all_ids: HashSet<String> = ours_map.keys()
        .chain(theirs_map.keys())
        .cloned()
        .collect();

    let mut merged = Vec::new();

    for id in all_ids {
        match (ours_map.get(&id), theirs_map.get(&id), ancestor_map.get(&id)) {
            // Both branches modified the record
            (Some(ours_rec), Some(theirs_rec), Some(_ancestor_rec)) => {
                // Pick the one with latest updated_at (last-write-wins)
                let ours_ts = extract_timestamp(ours_rec);
                let theirs_ts = extract_timestamp(theirs_rec);

                if ours_ts >= theirs_ts {
                    merged.push(ours_rec.clone());
                } else {
                    merged.push(theirs_rec.clone());
                }
            }
            // Only ours added/modified
            (Some(ours_rec), None, _) => {
                merged.push(ours_rec.clone());
            }
            // Only theirs added/modified
            (None, Some(theirs_rec), _) => {
                merged.push(theirs_rec.clone());
            }
            // Both deleted (rare, but handle gracefully)
            (None, None, Some(_)) => {
                // Don't include deleted records
            }
            // Unreachable (ID must be in at least one branch)
            (None, None, None) => unreachable!(),
        }
    }

    // 4. Sort by ID for deterministic output
    merged.sort_by(|a, b| {
        extract_id(a).cmp(&extract_id(b))
    });

    // 5. Serialize to JSONL
    let jsonl = merged.iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(jsonl)
}

/// Parse JSONL file into generic JSON values
fn parse_jsonl_generic(path: &Path) -> Result<Vec<Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let mut records = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(record) => records.push(record),
            Err(e) => {
                eprintln!("Warning: Failed to parse line {} in {}: {}", line_num + 1, path.display(), e);
            }
        }
    }

    Ok(records)
}

/// Build map of ID → Record, keeping only the latest occurrence
fn build_latest_map(records: Vec<Value>) -> HashMap<String, Value> {
    let mut map = HashMap::new();

    for record in records {
        let id = extract_id(&record);
        let ts = extract_timestamp(&record);

        match map.get(&id) {
            Some(existing) if extract_timestamp(existing) > ts => {
                // Keep existing (it's newer)
                continue;
            }
            _ => {
                // Insert or replace with this record (it's newer or first)
                map.insert(id, record);
            }
        }
    }

    map
}

fn extract_id(value: &Value) -> String {
    value.get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_timestamp(value: &Value) -> i64 {
    value.get("updated_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}
```

**Binary for git merge driver:**

```rust
// src/bin/taskstore-merge.rs

use taskstore::merge::merge_jsonl_files;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    if let Err(e) = run() {
        eprintln!("Merge failed: {}", e);
        std::process::exit(1);
    }
}

fn run() -> eyre::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 4 {
        eyre::bail!("Usage: taskstore-merge <ancestor> <ours> <theirs>");
    }

    let ancestor = Path::new(&args[1]);
    let ours = Path::new(&args[2]);
    let theirs = Path::new(&args[3]);

    // Perform three-way merge
    let merged = merge_jsonl_files(ancestor, ours, theirs)?;

    // Write result to "ours" file (git merge driver convention)
    fs::write(ours, merged)?;

    Ok(())
}
```

**Installation:**

```rust
impl Store {
    pub fn install_merge_driver(&self) -> Result<()> {
        // 1. Configure git to use our merge driver
        Command::new("git")
            .args(["config", "merge.taskstore-merge.name", "TaskStore JSONL merge driver"])
            .output()
            .context("Failed to set merge driver name")?;

        Command::new("git")
            .args(["config", "merge.taskstore-merge.driver", "taskstore-merge %O %A %B"])
            .output()
            .context("Failed to set merge driver command")?;

        // 2. Configure .gitattributes to use merge driver for JSONL files
        let gitattributes = ".taskstore/*.jsonl merge=taskstore-merge\n";
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(".gitattributes")
            .context("Failed to open .gitattributes")?;
        file.write_all(gitattributes.as_bytes())?;

        println!("✓ Installed custom merge driver for .taskstore/*.jsonl");

        Ok(())
    }
}
```

**Testing the merge driver:**

```bash
# Create test scenario
cd /tmp
mkdir merge-test && cd merge-test
git init

# Setup
mkdir .taskstore
# Initialize your application's store
taskstore install-hooks

# Create record on main
echo '{"id":"rec-001","title":"Feature A","updated_at":1000}' >> .taskstore/records.jsonl
git add .taskstore
git commit -m "Add record A"

# Branch and create different record
git checkout -b branch1
echo '{"id":"rec-002","title":"Feature B","updated_at":1001}' >> .taskstore/records.jsonl
git add .taskstore
git commit -m "Add record B"

# Back to main, create another record
git checkout main
echo '{"id":"rec-003","title":"Feature C","updated_at":1002}' >> .taskstore/records.jsonl
git add .taskstore
git commit -m "Add record C"

# Merge - should auto-resolve with no conflicts
git merge branch1

# Verify all three records exist
grep -c "^{" .taskstore/records.jsonl
# Expected: 3 (no conflicts!)
```

### 5.2. Git Hooks (IMPORTANT)

**What they do:** Automate sync operations to keep SQLite and JSONL in sync.

**Why they're needed:**
- Prevent database-JSONL inconsistencies
- No manual `taskstore sync` commands needed
- Works transparently with standard git workflows

**Complete hook set:**

```rust
impl Store {
    pub fn install_git_hooks(&self) -> Result<()> {
        self.install_hook("pre-commit", "taskstore sync")?;
        self.install_hook("post-merge", "taskstore sync")?;
        self.install_hook("post-rebase", "taskstore sync")?;
        self.install_hook("pre-push", "taskstore sync")?;
        self.install_hook("post-checkout", "taskstore sync")?;

        println!("✓ Installed git hooks (pre-commit, post-merge, post-rebase, pre-push, post-checkout)");

        Ok(())
    }

    fn install_hook(&self, hook_name: &str, command: &str) -> Result<()> {
        let hook_content = format!(
            "#!/bin/bash\n\
             # TaskStore auto-sync hook\n\
             cd \"$(git rev-parse --show-toplevel)\"\n\
             {} || true  # Don't fail git operation if sync fails\n\
             exit 0\n",
            command
        );

        let hook_path = PathBuf::from(".git/hooks").join(hook_name);

        // Append if hook already exists (don't overwrite user's hooks)
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&hook_path)
            .with_context(|| format!("Failed to open hook {}", hook_name))?;

        file.write_all(hook_content.as_bytes())?;

        // Make executable (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&hook_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms)?;
        }

        Ok(())
    }
}
```

**Hook purposes:**

| Hook | When It Runs | Purpose |
|------|--------------|---------|
| **pre-commit** | Before `git commit` | Ensure all mutations are flushed to JSONL before commit |
| **post-merge** | After `git merge` | Rebuild SQLite cache from merged JSONL |
| **post-rebase** | After `git rebase` | Rebuild SQLite cache from rebased JSONL |
| **pre-push** | Before `git push` | Final sync to ensure everything exported |
| **post-checkout** | After `git checkout` | Rebuild SQLite cache when switching branches |

**Why all 5 hooks:**
- `pre-commit`: Prevents committing stale JSONL (if using debounced export)
- `post-merge`: Imports changes from remote
- `post-rebase`: Imports changes after rebase
- `pre-push`: Safety check before pushing
- `post-checkout`: Branch switching needs cache rebuild

**Installation:**

```bash
# One-time setup per repo
taskstore install-hooks
```

**Usage:**

```bash
# One-time setup per repo
taskstore install-hooks

# Verify installation
ls -la .git/hooks/post-merge
cat .gitattributes
```

### 5.3. Debounced Export/Sync (PERFORMANCE)

**What it does:** Batches multiple mutations into a single JSONL write to improve performance.

**Why it's needed:**
```
Without debouncing:
  50 record creates in 5 seconds = 50 JSONL appends = 50 fsync calls = poor performance

With debouncing:
  50 record creates in 5 seconds = Wait 5s → 1 JSONL export = 1 fsync call = good performance
```

**When to use:**
- Batch operations (creating many records at once)
- Rapid mutations (importing data)
- Not needed for single operations (overhead not worth it)

**Implementation:**

```rust
// src/sync.rs

use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

/// Configuration for debounced sync
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Debounce interval in milliseconds (default: 5000)
    pub debounce_ms: u64,
    /// Auto-export on mutations (default: true)
    pub auto_export: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 5000,  // 5 seconds
            auto_export: true,
        }
    }
}

/// Sync manager for debounced JSONL export
pub struct SyncManager {
    dirty_tx: mpsc::UnboundedSender<DirtyRecord>,
}

#[derive(Debug, Clone)]
struct DirtyRecord {
    record_type: String,
    record_id: String,
}

impl SyncManager {
    pub fn new(store_path: PathBuf, config: SyncConfig) -> Self {
        let (dirty_tx, dirty_rx) = mpsc::unbounded_channel();

        // Spawn background task
        tokio::spawn(sync_worker(store_path, dirty_rx, config));

        Self { dirty_tx }
    }

    /// Mark a record as dirty (will be exported on next flush)
    pub fn mark_dirty(&self, record_type: String, record_id: String) {
        let _ = self.dirty_tx.send(DirtyRecord { record_type, record_id });
    }
}

/// Background worker that batches and exports dirty records
async fn sync_worker(
    store_path: PathBuf,
    mut dirty_rx: mpsc::UnboundedReceiver<DirtyRecord>,
    config: SyncConfig,
) {
    let mut dirty_records: HashMap<String, HashSet<String>> = HashMap::new();
    let mut timer = tokio::time::interval(Duration::from_millis(config.debounce_ms));

    loop {
        tokio::select! {
            Some(record) = dirty_rx.recv() => {
                dirty_records
                    .entry(record.record_type)
                    .or_default()
                    .insert(record.record_id);
                timer.reset();  // Reset debounce timer
            }
            _ = timer.tick() => {
                // Timer fired - export if dirty
                if !dirty_records.is_empty() {
                    export_dirty(&store_path, &dirty_records).await.ok();
                    dirty_records.clear();
                }
            }
        }
    }
}
```

**Usage:**

```rust
// Default (no debouncing)
let store = Store::open(path)?;

// With debouncing (5s default)
let store = Store::with_sync_config(path, SyncConfig::default())?;
```

## 6. Filtering and Querying

### Filter Types

```rust
// src/filter.rs

use serde::{Deserialize, Serialize};

/// Generic filter for querying records
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
    pub value: FilterValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    StartsWith,
    In,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterValue {
    String(String),
    Number(i64),
    List(Vec<String>),
}

impl Filter {
    /// Convert filter to SQL WHERE clause
    pub fn to_sql(&self) -> (String, Vec<rusqlite::types::Value>) {
        let clause = match self.op {
            FilterOp::Eq => format!("index_name = ? AND index_value = ?"),
            FilterOp::Ne => format!("index_name = ? AND index_value != ?"),
            FilterOp::Contains => format!("index_name = ? AND index_value LIKE ?"),
            FilterOp::In => {
                let placeholders = vec!["?"; self.value.as_list().len()].join(",");
                format!("index_name = ? AND index_value IN ({})", placeholders)
            }
            // ... other operators
        };

        let params = self.params();
        (clause, params)
    }
}
```

### Example Usage

```rust
use taskstore::{Store, Filter, FilterOp, FilterValue};

let store = Store::open(".taskstore")?;

// Get all active plans
let filter = Filter {
    field: "status".to_string(),
    op: FilterOp::Eq,
    value: FilterValue::String("active".to_string()),
};

let plans: Vec<Plan> = store.list(Some(filter))?;

// Get tasks for a specific plan
let filter = Filter {
    field: "plan_id".to_string(),
    op: FilterOp::Eq,
    value: FilterValue::String("plan-001".to_string()),
};

let tasks: Vec<Task> = store.list(Some(filter))?;

// Get all pending or blocked tasks
let filter = Filter {
    field: "status".to_string(),
    op: FilterOp::In,
    value: FilterValue::List(vec!["pending".to_string(), "blocked".to_string()]),
};

let tasks: Vec<Task> = store.list(Some(filter))?;
```

## 7. Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestRecord {
        id: String,
        value: String,
        updated_at: i64,
    }

    impl Record for TestRecord {
        fn record_type() -> RecordType {
            RecordType::new("test")
        }

        fn id(&self) -> &str {
            &self.id
        }

        fn updated_at(&self) -> i64 {
            self.updated_at
        }

        fn set_updated_at(&mut self, timestamp: i64) {
            self.updated_at = timestamp;
        }
    }

    #[test]
    fn test_create_and_get() {
        let mut store = Store::open_temp()?;

        let record = TestRecord {
            id: "test-001".to_string(),
            value: "Hello".to_string(),
            updated_at: 1000,
        };

        let id = store.create(record)?;
        assert_eq!(id, "test-001");

        let retrieved: Option<TestRecord> = store.get(&id)?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().value, "Hello");
    }

    #[test]
    fn test_sync_deduplicates() {
        let mut store = Store::open_temp()?;

        // Create multiple versions of same record
        let v1 = TestRecord { id: "test-001".to_string(), value: "v1".to_string(), updated_at: 1000 };
        let v2 = TestRecord { id: "test-001".to_string(), value: "v2".to_string(), updated_at: 2000 };
        let v3 = TestRecord { id: "test-001".to_string(), value: "v3".to_string(), updated_at: 3000 };

        store.append_jsonl("test.jsonl", &v1)?;
        store.append_jsonl("test.jsonl", &v2)?;
        store.append_jsonl("test.jsonl", &v3)?;

        // Sync should keep only latest
        store.sync()?;

        let record: Option<TestRecord> = store.get("test-001")?;
        assert_eq!(record.unwrap().value, "v3");  // v3 is latest
    }

    #[test]
    fn test_list_with_filter() {
        let mut store = Store::open_temp()?;

        store.create(TestRecord { id: "test-001".to_string(), value: "active".to_string(), updated_at: 1000 })?;
        store.create(TestRecord { id: "test-002".to_string(), value: "inactive".to_string(), updated_at: 1000 })?;
        store.create(TestRecord { id: "test-003".to_string(), value: "active".to_string(), updated_at: 1000 })?;

        let filter = Filter {
            field: "value".to_string(),
            op: FilterOp::Eq,
            value: FilterValue::String("active".to_string()),
        };

        let results: Vec<TestRecord> = store.list(Some(filter))?;
        assert_eq!(results.len(), 2);
    }
}
```

### Integration Tests

```rust
#[test]
fn test_merge_driver() {
    // Create conflicting JSONL files
    let base = "base.jsonl";
    let ours = "ours.jsonl";
    let theirs = "theirs.jsonl";

    fs::write(base, r#"{"id":"rec-001","value":"base","updated_at":1000}"#)?;
    fs::write(ours, r#"{"id":"rec-001","value":"ours","updated_at":2000}"#)?;
    fs::write(theirs, r#"{"id":"rec-001","value":"theirs","updated_at":1500}"#)?;

    // Simulate merge
    let result = merge_jsonl_files(base, ours, theirs)?;

    // Verify correct resolution (ours wins - latest timestamp)
    assert!(result.contains("ours"));
}

#[test]
fn test_git_hook_sync() {
    // Initialize repo with taskstore
    let repo = init_test_repo()?;
    let mut store = Store::open(repo.path())?;
    store.install_git_integration()?;

    // Simulate post-merge
    run_hook(&repo, "post-merge")?;

    // Verify sync was called
    assert!(store.is_synced()?);
}
```

## 8. CLI Commands

**Full command set:**

```bash
# List operations
taskstore list <type>                # List all records of type
taskstore list <type> --filter "status=active"  # Filter results

# Show operations
taskstore show <id>                  # Show record by ID
taskstore describe <id>              # Show full details (if applicable)

# Maintenance
taskstore sync                       # Rebuild SQLite from JSONL
taskstore compact                    # Remove superseded JSONL records
taskstore check                      # Validate consistency

# Git integration
taskstore install-hooks              # Install git hooks and merge driver
taskstore merge <base> <ours> <theirs>  # Merge driver (internal)

# Export
taskstore backup <dest>              # Copy JSONL files
taskstore export-json                # Export all data as JSON
```

## 9. Performance Considerations

### Optimization Strategies

1. **SQLite WAL mode:**
```rust
conn.execute_batch("PRAGMA journal_mode=WAL")?;
```

2. **Batch writes:**
```rust
let tx = conn.transaction()?;
for record in records {
    tx.execute("INSERT INTO ...", ...)?;
}
tx.commit()?;
```

3. **Streaming JSONL reads:**
```rust
// Don't load entire file into memory
let file = BufReader::new(File::open(path)?);
for line in file.lines() {
    let record: Value = serde_json::from_str(&line?)?;
    process(record)?;
}
```

4. **Lazy index building:**
```rust
// Only build indexes for fields that are actually queried
pub fn ensure_index(&mut self, record_type: &str, field: &str) -> Result<()> {
    if !self.has_index(record_type, field)? {
        self.rebuild_index(record_type, field)?;
    }
    Ok(())
}
```

5. **Selective field extraction:**
```rust
// Don't deserialize full records if you only need ID
pub fn list_ids(&self, record_type: &str) -> Result<Vec<String>> {
    let mut stmt = self.db.prepare(
        "SELECT id FROM records WHERE type = ?1"
    )?;

    let ids = stmt.query_map(params![record_type], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;

    Ok(ids)
}
```

## 10. Error Handling Patterns

```rust
// Use eyre for context
pub fn create<T: Record>(&mut self, record: T) -> Result<String> {
    // Validate
    if record.id().is_empty() {
        return Err(eyre!("Record ID cannot be empty"));
    }

    // Write JSONL first (source of truth)
    self.append_jsonl(&T::record_type().jsonl_file, &record)
        .wrap_err_with(|| format!("Failed to write {} to JSONL", record.id()))?;

    // Then SQLite
    self.insert_record(&record)
        .wrap_err_with(|| format!("Failed to insert {} into SQLite", record.id()))?;

    Ok(record.id().to_string())
}
```

## 11. Example Application

Here's a complete example of building a simple project management system with TaskStore:

```rust
// my-app/src/main.rs

use taskstore::{Store, Record, RecordType, Filter, FilterOp, FilterValue};
use serde::{Deserialize, Serialize};
use eyre::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Plan {
    id: String,
    title: String,
    description: String,
    status: String,
    created_at: i64,
    updated_at: i64,
}

impl Record for Plan {
    fn record_type() -> RecordType { RecordType::new("plans") }
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn set_updated_at(&mut self, ts: i64) { self.updated_at = ts; }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    id: String,
    plan_id: String,
    title: String,
    status: String,
    created_at: i64,
    updated_at: i64,
}

impl Record for Task {
    fn record_type() -> RecordType { RecordType::new("tasks") }
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn set_updated_at(&mut self, ts: i64) { self.updated_at = ts; }
}

fn main() -> Result<()> {
    let mut store = Store::open(".myapp")?;

    // Create a plan
    let plan = Plan {
        id: "plan-001".to_string(),
        title: "Launch MVP".to_string(),
        description: "Build and launch minimum viable product".to_string(),
        status: "active".to_string(),
        created_at: 1704067200000,
        updated_at: 1704067200000,
    };

    store.create(plan)?;

    // Create tasks
    let task1 = Task {
        id: "task-001".to_string(),
        plan_id: "plan-001".to_string(),
        title: "Design UI mockups".to_string(),
        status: "pending".to_string(),
        created_at: 1704067200000,
        updated_at: 1704067200000,
    };

    store.create(task1)?;

    // Query tasks for a plan
    let filter = Filter {
        field: "plan_id".to_string(),
        op: FilterOp::Eq,
        value: FilterValue::String("plan-001".to_string()),
    };

    let tasks: Vec<Task> = store.list(Some(filter))?;

    println!("Tasks for plan-001:");
    for task in tasks {
        println!("  - {} [{}]", task.title, task.status);
    }

    Ok(())
}
```

## 12. Best Practices

### 1. Define Clear Record Types

Keep your record types focused and well-defined:

```rust
// Good: Focused, single responsibility
struct User { id, name, email, ... }
struct Event { id, user_id, type, ... }

// Bad: Kitchen sink, too many concerns
struct UserWithEventsAndSettings { ... }
```

### 2. Use Consistent Timestamps

Always use milliseconds since epoch (i64):

```rust
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
```

### 3. Index Only What You Query

Don't index every field - only the ones you actually filter on:

```rust
// Good: Index fields you query
store.register_index::<Task>("plan_id")?;
store.register_index::<Task>("status")?;

// Bad: Indexing fields you never query
store.register_index::<Task>("created_at")?;  // If you never filter by this
```

### 4. Use Meaningful IDs

Generate IDs that are sortable and contain type information:

```rust
use uuid::Uuid;

fn new_id(prefix: &str) -> String {
    format!("{}-{}", prefix, Uuid::now_v7())
}

// Results in: plan-01932abc-def0-7890-1234-567890abcdef
let id = new_id("plan");
```

### 5. Handle Errors Gracefully

Provide context in error messages:

```rust
store.create(plan)
    .wrap_err_with(|| format!("Failed to create plan: {}", plan.title))?;
```

## 13. Migration from Other Storage Systems

### From SQL Database

```rust
// Read from old SQL database
let plans = sqlx::query_as::<_, OldPlan>("SELECT * FROM plans")
    .fetch_all(&pool)
    .await?;

// Convert and write to TaskStore
let mut store = Store::open(".taskstore")?;
for old_plan in plans {
    let new_plan = Plan {
        id: format!("plan-{}", old_plan.id),
        title: old_plan.name,
        // ... map fields
        updated_at: now_ms(),
    };

    store.create(new_plan)?;
}
```

### From JSON Files

```rust
// Read from old JSON files
let data: Vec<OldRecord> = serde_json::from_str(&fs::read_to_string("data.json")?)?;

// Convert and write to TaskStore
let mut store = Store::open(".taskstore")?;
for old_record in data {
    let new_record = convert_record(old_record);
    store.create(new_record)?;
}
```

## 14. References

- [Storage Architecture](./storage-architecture.md) - Architecture overview
- [TaskStore Design](./taskstore-design.md) - Detailed design document
- SQLite documentation: https://www.sqlite.org/
- JSONL format: https://jsonlines.org/
- Git merge driver: https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver
