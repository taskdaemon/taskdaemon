# TaskStore Storage Architecture: The Bead Store Pattern

**Date:** 2026-01-13
**Author:** Claude Sonnet 4.5
**Status:** Authoritative

## Summary

TaskStore implements the **Bead Store pattern** from Steve Yegge's Gas Town: records stored as one-line JSON in Git-backed JSONL files, with SQLite as a rebuildable query cache. This provides durability, version control, collaboration, and crash recovery while maintaining fast query performance.

TaskStore is a **generic infrastructure library** that can store any type implementing the `Record` trait.

## The Bead Store Pattern (from Gas Town)

Steve Yegge's Gas Town stores work items as "Beads":
- **Beads** = Atomic work units with persistent identity (like GitHub issues)
- **Storage** = One JSON record per line in `.jsonl` files, committed to Git
- **Query** = SQLite index rebuilt from JSONL for fast lookups
- **Updates** = Append-only (multiple lines for same ID, latest wins)

**Key insight:** Beads are **work items**, not just data. They track state, identity, relationships, and changes over time.

## Core Concepts

### The Record Trait

TaskStore is designed to store any type that implements the `Record` trait:

```rust
pub trait Record: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    /// Unique identifier for this record
    fn id(&self) -> &str;

    /// Timestamp when this record was last updated (Unix epoch milliseconds)
    fn updated_at(&self) -> i64;

    /// Type name for this record (used for JSONL file routing)
    fn type_name() -> &'static str where Self: Sized;
}
```

**Examples of Record types:**
- `Plan` - A high-level project or goal
- `Task` - A discrete unit of work
- `User` - A person or agent in the system
- `Event` - A state change or notification
- `Message` - Communication between entities

### Two Storage Layers

```
┌─────────────────────────────────────────────────────────────┐
│                    LAYER 1: JSONL Files                      │
│                  (Source of Truth, in Git)                   │
│                                                              │
│  .taskstore/                                                 │
│  ├── plans.jsonl          ← Example: High-level plans       │
│  ├── tasks.jsonl          ← Example: Work items             │
│  ├── users.jsonl          ← Example: User records           │
│  └── events.jsonl         ← Example: System events          │
│                                                              │
│  Properties:                                                 │
│  - Append-only (never delete lines)                         │
│  - One JSON object per line                                 │
│  - Multiple lines for same ID allowed                       │
│  - Latest record wins (highest updated_at)                  │
│  - Committed to Git (version controlled)                    │
│  - Human-readable (can grep, inspect)                       │
└─────────────────────────────────────────────────────────────┘
                            │
                            │ sync() rebuilds
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                  LAYER 2: SQLite Database                    │
│                  (Query Cache, ephemeral)                    │
│                                                              │
│  .taskstore/taskstore.db (in .gitignore)                    │
│                                                              │
│  Tables:                                                     │
│  - records (generic record storage)                          │
│  - record_indexes (per-type indexed fields)                  │
│                                                              │
│  Properties:                                                 │
│  - Fast indexed queries                                      │
│  - Built from JSONL on startup                              │
│  - Can be deleted and rebuilt anytime                       │
│  - NOT committed to Git                                     │
└─────────────────────────────────────────────────────────────┘
```

## Generic Schema

TaskStore uses a generic schema that works with any Record type:

### records table
```sql
CREATE TABLE records (
    id TEXT PRIMARY KEY,
    record_type TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    data TEXT NOT NULL  -- Full JSON of the record
);

CREATE INDEX idx_records_type ON records(record_type);
CREATE INDEX idx_records_updated ON records(updated_at);
```

### record_indexes table (optional, for custom queries)
```sql
CREATE TABLE record_indexes (
    record_id TEXT NOT NULL,
    record_type TEXT NOT NULL,
    field_name TEXT NOT NULL,
    field_value TEXT NOT NULL,
    FOREIGN KEY(record_id) REFERENCES records(id)
);

CREATE INDEX idx_record_indexes_type_field ON record_indexes(record_type, field_name, field_value);
```

## Usage Examples

### Example 1: Storing Plans and Tasks

```rust
#[derive(Serialize, Deserialize, Clone)]
struct Plan {
    id: String,
    title: String,
    status: String,
    updated_at: i64,
}

impl Record for Plan {
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn type_name() -> &'static str { "plan" }
}

#[derive(Serialize, Deserialize, Clone)]
struct Task {
    id: String,
    plan_id: String,
    name: String,
    status: String,
    updated_at: i64,
}

impl Record for Task {
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn type_name() -> &'static str { "task" }
}
```

**JSONL Example (plans.jsonl):**
```jsonl
{"id":"plan-001","title":"Build Authentication System","status":"active","updated_at":1704067200000}
{"id":"plan-002","title":"Add Payment Integration","status":"planning","updated_at":1704067300000}
```

**JSONL Example (tasks.jsonl):**
```jsonl
{"id":"task-001","plan_id":"plan-001","name":"Implement JWT tokens","status":"pending","updated_at":1704067200000}
{"id":"task-002","plan_id":"plan-001","name":"Add login endpoint","status":"pending","updated_at":1704067210000}
{"id":"task-001","plan_id":"plan-001","name":"Implement JWT tokens","status":"in_progress","updated_at":1704067800000}
{"id":"task-001","plan_id":"plan-001","name":"Implement JWT tokens","status":"complete","updated_at":1704070400000}
```

Note: task-001 appears three times with different statuses. This is the append-only pattern - latest wins.

### Example 2: Storing Users and Events

```rust
#[derive(Serialize, Deserialize, Clone)]
struct User {
    id: String,
    name: String,
    email: String,
    active: bool,
    updated_at: i64,
}

impl Record for User {
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn type_name() -> &'static str { "user" }
}

#[derive(Serialize, Deserialize, Clone)]
struct Event {
    id: String,
    user_id: String,
    event_type: String,
    payload: String,
    updated_at: i64,
}

impl Record for Event {
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn type_name() -> &'static str { "event" }
}
```

**JSONL Example (users.jsonl):**
```jsonl
{"id":"user-001","name":"Alice","email":"alice@example.com","active":true,"updated_at":1704067200000}
{"id":"user-002","name":"Bob","email":"bob@example.com","active":true,"updated_at":1704067300000}
{"id":"user-001","name":"Alice","email":"alice@newdomain.com","active":true,"updated_at":1704070000000}
```

**JSONL Example (events.jsonl):**
```jsonl
{"id":"evt-001","user_id":"user-001","event_type":"login","payload":"{\"ip\":\"192.168.1.1\"}","updated_at":1704067500000}
{"id":"evt-002","user_id":"user-001","event_type":"task_complete","payload":"{\"task_id\":\"task-001\"}","updated_at":1704070400000}
```

## Write Operations

### Creating a Record

```rust
pub fn create<T: Record>(&mut self, record: T) -> Result<String> {
    // 1. Append to JSONL (durability first)
    let type_name = T::type_name();
    let json = serde_json::to_string(&record)?;
    let path = self.store_dir.join(format!("{}.jsonl", type_name));

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", json)?;
    file.sync_all()?;  // fsync to disk

    // 2. Insert into SQLite (query performance)
    self.db.execute(
        "INSERT INTO records (id, record_type, updated_at, data)
         VALUES (?1, ?2, ?3, ?4)",
        params![record.id(), type_name, record.updated_at(), json],
    )?;

    Ok(record.id().to_string())
}
```

### Updating a Record (Append-Only)

```rust
pub fn update<T: Record>(&mut self, record: T) -> Result<()> {
    // 1. Append to JSONL (yes, duplicate ID - that's the pattern!)
    let type_name = T::type_name();
    let json = serde_json::to_string(&record)?;
    let path = self.store_dir.join(format!("{}.jsonl", type_name));

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", json)?;
    file.sync_all()?;

    // 2. Update SQLite (single record)
    self.db.execute(
        "UPDATE records
         SET updated_at = ?1, data = ?2
         WHERE id = ?3 AND record_type = ?4",
        params![record.updated_at(), json, record.id(), type_name],
    )?;

    Ok(())
}
```

**Result:** JSONL has multiple lines for same ID, each with different data and updated_at.

## Read Operations

### Fast Queries (SQLite)

```rust
pub fn get<T: Record>(&self, id: &str) -> Result<Option<T>> {
    let type_name = T::type_name();

    let mut stmt = self.db.prepare(
        "SELECT data FROM records WHERE id = ?1 AND record_type = ?2"
    )?;

    let result = stmt.query_row(params![id, type_name], |row| {
        let json: String = row.get(0)?;
        Ok(serde_json::from_str(&json).unwrap())
    });

    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn list<T: Record>(&self) -> Result<Vec<T>> {
    let type_name = T::type_name();

    let mut stmt = self.db.prepare(
        "SELECT data FROM records WHERE record_type = ?1 ORDER BY updated_at"
    )?;

    let records = stmt.query_map([type_name], |row| {
        let json: String = row.get(0)?;
        Ok(serde_json::from_str(&json).unwrap())
    })?
    .collect::<Result<Vec<_>, _>>()?;

    Ok(records)
}
```

### Custom Filtering

```rust
// Using record_indexes for efficient filtering
pub fn find_by_field(&self, record_type: &str, field_name: &str, field_value: &str) -> Result<Vec<String>> {
    let mut stmt = self.db.prepare(
        "SELECT r.data FROM records r
         JOIN record_indexes ri ON r.id = ri.record_id
         WHERE ri.record_type = ?1
           AND ri.field_name = ?2
           AND ri.field_value = ?3"
    )?;

    let records = stmt.query_map(params![record_type, field_name, field_value], |row| {
        row.get(0)
    })?
    .collect::<Result<Vec<_>, _>>()?;

    Ok(records)
}
```

### Rebuild from JSONL (Sync)

```rust
pub fn sync(&mut self) -> Result<()> {
    // 1. Read all JSONL files
    for entry in fs::read_dir(&self.store_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension() != Some(OsStr::new("jsonl")) {
            continue;
        }

        let records = self.read_jsonl(&path)?;

        // 2. Deduplicate: keep latest record per ID (highest updated_at)
        let mut latest: HashMap<String, (i64, String)> = HashMap::new();
        for (id, updated_at, json) in records {
            match latest.get(&id) {
                Some((existing_ts, _)) if *existing_ts > updated_at => continue,
                _ => { latest.insert(id, (updated_at, json)); }
            }
        }

        // 3. Extract record type from filename
        let record_type = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::msg("Invalid filename"))?;

        // 4. Upsert into SQLite
        for (id, (updated_at, json)) in latest {
            self.db.execute(
                "INSERT OR REPLACE INTO records (id, record_type, updated_at, data)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, record_type, updated_at, json],
            )?;
        }
    }

    Ok(())
}

fn read_jsonl(&self, path: &Path) -> Result<Vec<(String, i64, String)>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let value: serde_json::Value = serde_json::from_str(&line)?;

        let id = value["id"].as_str()
            .ok_or_else(|| Error::msg("Missing id field"))?
            .to_string();
        let updated_at = value["updated_at"].as_i64()
            .ok_or_else(|| Error::msg("Missing updated_at field"))?;

        records.push((id, updated_at, line));
    }

    Ok(records)
}
```

## Git Integration

### Committing Records

```bash
# Checkpoint your data
cd /path/to/repo
git add .taskstore/*.jsonl
git commit -m "Checkpoint: updated 15 tasks, added 3 users"

# JSONL files might have hundreds of lines for dozens of records
# (each record updated multiple times)
# That's fine - it's append-only history
```

### Pulling Updates

```bash
git pull origin main

# post-merge hook automatically runs:
#!/bin/bash
cd "$(git rev-parse --show-toplevel)"
taskstore sync  # Rebuilds SQLite from merged JSONL
```

### Merge Conflicts

Custom merge driver (`.gitattributes`):
```
.taskstore/*.jsonl merge=taskstore-merge
```

**taskstore-merge algorithm:**
1. Parse base, ours, theirs (all JSONL)
2. Combine all records from both sides
3. Deduplicate by ID (keep record with highest updated_at)
4. Write merged JSONL
5. Exit 0 (success)

**Example conflict:**
```
Base:    {"id":"task-123","status":"pending","updated_at":1000}
Ours:    {"id":"task-123","status":"in_progress","updated_at":1001}
Theirs:  {"id":"task-123","status":"complete","updated_at":1002}

Merged:  {"id":"task-123","status":"complete","updated_at":1002}  # Theirs wins (newer)
```

## Compaction (Optional)

Over time, JSONL files grow (multiple lines per ID). Compaction removes superseded records:

```bash
taskstore compact

# Or automatically when file > 100MB
```

**Algorithm:**
```rust
pub fn compact(&mut self, type_name: &str) -> Result<()> {
    let path = self.store_dir.join(format!("{}.jsonl", type_name));

    // 1. Read all records
    let records = self.read_jsonl(&path)?;

    // 2. Keep only latest per ID
    let mut latest: HashMap<String, (i64, String)> = HashMap::new();
    for (id, updated_at, json) in records {
        match latest.get(&id) {
            Some((existing_ts, _)) if *existing_ts > updated_at => continue,
            _ => { latest.insert(id, (updated_at, json)); }
        }
    }

    // 3. Write to temp file
    let temp_path = path.with_extension("jsonl.tmp");
    let mut temp = File::create(&temp_path)?;
    for (_, (_, json)) in latest.iter() {
        writeln!(temp, "{}", json)?;
    }
    temp.sync_all()?;

    // 4. Atomic rename
    fs::rename(temp_path, path)?;

    Ok(())
}
```

**Result:** JSONL shrinks from 5000 lines to 50 lines (50 records × 100 updates = 5000, compacted to 50 latest)

## Crash Recovery

### Scenario:

```
12:00 PM - Application starts
12:01 PM - Create 3 tasks
          → Written to tasks.jsonl
12:05 PM - Update task-001 to "in_progress"
          → Appended to tasks.jsonl
12:10 PM - Update task-002 to "in_progress"
          → Appended to tasks.jsonl
12:15 PM - Update task-001 to "complete"
          → Appended to tasks.jsonl
12:16 PM - **CRASH** (power failure)
```

### On Restart:

```
12:20 PM - Application restarts
          → Calls store.sync()
          → Reads tasks.jsonl (6 lines)
          → Deduplicates to 3 latest records
          → Rebuilds SQLite

12:21 PM - Queries SQLite: "SELECT * FROM records WHERE record_type='task'"
          → Finds 3 tasks with correct states:
            - task-001: "complete"
            - task-002: "in_progress"
            - task-003: "pending"

12:22 PM - Application continues from last known state

Data integrity maintained!
```

## Benefits of the Bead Store Pattern

1. **Durability:** JSONL writes are fsync'd, survive crashes
2. **Version Control:** Full git history of all records
3. **Collaboration:** Multiple users see same data
4. **Audit Trail:** Every state change recorded
5. **Crash Recovery:** Resume exactly where we left off
6. **Fast Queries:** SQLite indexes for performance
7. **Simple Consistency:** JSONL is truth, SQLite is cache
8. **Human Readable:** Can grep/inspect JSONL files directly
9. **Merge Capability:** Git merges handled automatically
10. **Flexible:** Append-only means never delete history
11. **Generic:** Works with any type implementing Record trait

## Anti-Patterns to Avoid

❌ **Don't put SQLite in Git**
- Binary file, git diffs useless
- Changes constantly, huge repo bloat
- Can rebuild from JSONL anytime

❌ **Don't update JSONL in-place**
- Append only! Multiple lines per ID is the pattern
- Compaction is a separate operation

❌ **Don't query JSONL directly for speed**
- Use SQLite for queries
- JSONL is for durability and git

❌ **Don't trust SQLite as source of truth**
- If JSONL and SQLite disagree, JSONL wins
- Always rebuild from JSONL on conflict

❌ **Don't forget to implement Record trait correctly**
- Must have unique, stable IDs
- Must have monotonically increasing updated_at
- Must implement all required trait methods

## Extension Points

### Custom Indexes

For frequently-queried fields, populate the `record_indexes` table:

```rust
pub fn create_index<T: Record>(&mut self, record: &T, field_name: &str, field_value: &str) -> Result<()> {
    self.db.execute(
        "INSERT INTO record_indexes (record_id, record_type, field_name, field_value)
         VALUES (?1, ?2, ?3, ?4)",
        params![record.id(), T::type_name(), field_name, field_value],
    )?;
    Ok(())
}
```

### Custom Filters

Implement type-specific filtering logic:

```rust
pub trait Filter<T: Record> {
    fn matches(&self, record: &T) -> bool;
}

pub fn filter<T: Record, F: Filter<T>>(&self, filter: F) -> Result<Vec<T>> {
    let all_records = self.list::<T>()?;
    Ok(all_records.into_iter().filter(|r| filter.matches(r)).collect())
}
```

### Validation

Add validation hooks before writes:

```rust
pub trait Validator<T: Record> {
    fn validate(&self, record: &T) -> Result<()>;
}

pub fn create_with_validation<T: Record, V: Validator<T>>(
    &mut self,
    record: T,
    validator: V
) -> Result<String> {
    validator.validate(&record)?;
    self.create(record)
}
```

## References

- Gas Town Beads: `~/.config/pais/tech/researcher/steve-yegge-gas-town-2026-01-13.md`
- TaskStore Design: `./taskstore-design.md`
- JSONL Format: https://jsonlines.org/
