# Design Document: TaskStore Implementation Readiness Fixes

**Author:** Claude (Opus 4.5)
**Date:** 2026-01-15
**Status:** Implemented
**Review Passes:** 5/5

## Summary

This design addresses critical fixes required for TaskStore before it can be used reliably by TaskDaemon. The fixes include: adding file locking for concurrent write protection, adding a `rebuild_indexes<T>()` method for index restoration after sync, improving staleness detection, and adding ID validation.

## Problem Statement

### Background

TaskStore is a generic persistent state management library that uses JSONL files as the source of truth with SQLite as a query cache. It was designed to support TaskDaemon's state management needs.

An [Implementation Readiness Assessment](../../../taskdaemon/docs/implementation-readiness-assessment.md) reviewed TaskStore and identified several issues that must be fixed before production use. This design document addresses those findings.

### Problem

The current TaskStore implementation has the following critical issues:

1. **No file locking**: Concurrent writes from multiple processes can corrupt JSONL files
2. **Indexes not restored during sync** (`store.rs:501-503`): After syncing from JSONL, the `record_indexes` table is empty until writes occur
3. **Incomplete staleness check** (`store.rs:122-145`): Only checks if records table is empty, won't detect new JSONL files added after initial sync
4. **No ID validation**: Empty string IDs are accepted, which could cause issues
5. **Memory concerns with large files**: `read_jsonl_latest()` loads entire file into memory

### Goals

- Fix all blocking issues preventing TaskStore from building and running correctly
- Ensure data integrity under concurrent access from multiple processes
- Restore full query functionality after sync operations
- Correctly detect when re-sync is needed
- Add defensive validation to prevent bad data

### Non-Goals

- Performance optimization (out of scope for this design)
- Adding new user-facing features beyond fixing existing gaps
- Changing the JSONL file format
- Implementing the StateManager actor wrapper (that's Phase 2)

## Proposed Solution

### Overview

Four discrete fixes, each independently testable:

| Fix | Section | Description |
|-----|---------|-------------|
| 1 | Fix 1 | Add advisory file locking using `fs2` crate |
| 2 | Fix 2 | Add `rebuild_indexes<T>()` method to restore indexes after sync |
| 3 | Fix 3 | Improve staleness detection using file modification times |
| 4 | Fix 4 | Add ID validation (non-empty, max length) |

**Note:** Large file handling (item 5 in original assessment) is analyzed and deferred - see Fix 5 section.

**Architecture alignment:** These fixes maintain TaskStore's existing patterns:
- `validate_id()` follows the existing `validate_collection_name()` / `validate_field_name()` pattern
- `rebuild_indexes<T>()` follows the existing generic `list<T>()` / `create<T>()` API pattern
- `sync_metadata` table follows existing schema conventions
- File locking is an internal implementation detail, no API changes

**StateManager integration (Phase 2):** When TaskDaemon wraps TaskStore in a StateManager actor:
- StateManager will call `rebuild_indexes<T>()` for each known type after `sync()` completes
- This is a clean integration point - StateManager knows all record types at compile time
- No changes to this design are needed for StateManager compatibility

### Fix 1: File Locking

**New Dependency:**
```toml
fs2 = "0.4"
```

**Approach:** Use advisory file locks via the `fs2` crate. Lock the JSONL file during append operations to prevent concurrent writes from corrupting data.

**Implementation in `store.rs`:**

```rust
use fs2::FileExt;

fn append_jsonl_generic<T: Record>(&self, collection: &str, record: &T) -> Result<()> {
    let jsonl_path = self.base_path.join(format!("{}.jsonl", collection));

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&jsonl_path)
        .context("Failed to open JSONL file for appending")?;

    // Acquire exclusive lock before writing
    file.lock_exclusive()
        .context("Failed to acquire file lock")?;

    let json = serde_json::to_string(record)?;

    use std::io::Write;
    writeln!(file, "{}", json)?;
    file.sync_all()?;

    // Lock is automatically released when file is dropped
    Ok(())
}
```

**Same pattern for:**
- `append_jsonl_raw()` in `store.rs`
- `append_jsonl()` in `jsonl.rs`

**For read operations during sync:**
- Use shared lock (`lock_shared()`) to allow concurrent reads while blocking writes

**Atomicity note:** The lock covers only the JSONL append, not the subsequent SQLite insert. This means:
- Two concurrent `create()` calls may interleave: A writes JSONL, B writes JSONL, A writes SQLite, B writes SQLite
- This is acceptable because JSONL is source of truth and SQLite is a cache
- The `updated_at` field ensures correct ordering when syncing from JSONL
- A full transaction lock would require a separate lockfile, adding complexity for minimal benefit

### Fix 2: Index Restoration During Sync

**Problem:** The sync operation (`store.rs:459-508`) clears all tables and reloads records, but the comment at lines 501-503 notes:

```rust
// Note: We don't restore indexes during sync since we don't know
// which fields were indexed. This is a limitation of the generic approach.
// Indexes will be rebuilt on next write operation.
```

**Solution:** Since we're syncing raw JSON values (not typed records), we cannot call `indexed_fields()`. Instead, we need a two-phase approach:

1. During sync, store only the records (current behavior)
2. After sync completes, the application must call a new `rebuild_indexes<T>()` method for each record type it uses

**New API:**

```rust
impl Store {
    /// Rebuild indexes for a specific record type after sync
    ///
    /// Call this for each record type after sync() completes. The method:
    /// - Reads all records from SQLite for the collection
    /// - Deserializes each to type T to extract indexed_fields()
    /// - Rebuilds the record_indexes table entries
    ///
    /// Returns the number of records successfully indexed.
    ///
    /// # Edge case handling
    /// If records in the collection don't deserialize to type T (e.g., wrong type
    /// passed), those records are skipped with a warning log. This prevents crashes
    /// while alerting to potential misconfiguration.
    pub fn rebuild_indexes<T: Record>(&mut self) -> Result<usize> {
        let collection = T::collection_name();

        // Get raw JSON from SQLite (bypass list<T> to handle deserialization errors)
        let mut stmt = self.db.prepare(
            "SELECT id, data_json FROM records WHERE collection = ?1"
        )?;

        let rows = stmt.query_map([collection], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let tx = self.db.transaction()?;
        let mut count = 0;

        for row_result in rows {
            let (id, data_json) = row_result?;

            // Attempt deserialization - skip records that don't match type T
            let record: T = match serde_json::from_str(&data_json) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        collection = collection,
                        id = &id,
                        error = ?e,
                        "Skipping record that doesn't match type"
                    );
                    continue;
                }
            };

            Self::update_indexes_tx(&tx, collection, &id, &record.indexed_fields())?;
            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }
}
```

**Alternative considered:** Store raw JSON in record_indexes during sync, then parse on query. Rejected because it would require schema changes and add query complexity.

### Fix 3: Improved Staleness Detection

**Problem:** Current `is_stale()` only returns `true` if JSONL files exist AND records table is empty. This fails to detect:
- New JSONL files added after initial sync
- JSONL files modified after sync (e.g., by git merge)

**Solution:** Track sync metadata in a new table.

**Schema addition in `create_schema()`:**

```rust
// Add to create_schema() method:
self.db.execute_batch(
    r#"
    -- Sync metadata for staleness detection
    CREATE TABLE IF NOT EXISTS sync_metadata (
        collection TEXT PRIMARY KEY,
        last_sync_time INTEGER NOT NULL,
        file_mtime INTEGER NOT NULL
    );
    "#,
)?;
```

**New `is_stale()` implementation:**

```rust
pub fn is_stale(&self) -> Result<bool> {
    // Check each JSONL file
    for entry in fs::read_dir(&self.base_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        let collection = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| eyre!("Invalid JSONL filename"))?;

        // Get file modification time
        let metadata = fs::metadata(&path)?;
        let file_mtime = metadata.modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        // Check if we have sync metadata for this collection
        let stored_mtime: Option<i64> = self.db
            .query_row(
                "SELECT file_mtime FROM sync_metadata WHERE collection = ?1",
                [collection],
                |row| row.get(0)
            )
            .optional()?;

        match stored_mtime {
            None => return Ok(true),  // Never synced
            Some(mtime) if file_mtime > mtime => return Ok(true),  // File modified
            _ => continue,
        }
    }

    Ok(false)
}
```

**Update sync() to record metadata:**

```rust
// After syncing a collection:
self.db.execute(
    "INSERT OR REPLACE INTO sync_metadata (collection, last_sync_time, file_mtime)
     VALUES (?1, ?2, ?3)",
    rusqlite::params![collection, now_ms(), file_mtime],
)?;
```

**Edge case: mtime resolution**

Some filesystems have coarse mtime resolution (1 second on HFS+, ext3). Two rapid writes within the same second would have identical mtimes, potentially missing the second write during staleness detection.

**Mitigation:** This is acceptable because:
- Git operations (merge, checkout, rebase) typically touch files with distinct mtimes
- Same-second writes from the same process don't need staleness detection (they're in the same session)
- Worst case: a sync is missed, user can force with `store.sync()`

**Edge case: deleted JSONL file**

If a JSONL file is deleted but sync_metadata still references it, `is_stale()` won't detect the deletion (no file to check).

**Mitigation:** Add cleanup in `sync()`:
```rust
// After processing all JSONL files, remove orphaned metadata:
self.db.execute(
    "DELETE FROM sync_metadata WHERE collection NOT IN (SELECT DISTINCT collection FROM records)",
    [],
)?;
```

### Fix 4: ID Validation

**Problem:** Empty string IDs are accepted, which could cause key collisions or lookup failures.

**Solution:** Add validation in two places:

1. **In `Record` trait documentation** - document the requirement
2. **In `Store::create()`** - validate before storing

```rust
/// Validate record ID
fn validate_id(id: &str) -> Result<()> {
    // Check not empty or whitespace-only
    if id.trim().is_empty() {
        return Err(eyre!("Record ID cannot be empty or whitespace-only"));
    }

    // Check reasonable length (prevent DoS via huge IDs)
    if id.len() > 256 {
        return Err(eyre!("Record ID too long: {} chars (max 256)", id.len()));
    }

    Ok(())
}

pub fn create<T: Record>(&mut self, record: T) -> Result<String> {
    let collection = T::collection_name();
    Self::validate_collection_name(collection)?;

    let id = record.id().to_string();
    Self::validate_id(&id)?;

    // ... rest of method
}
```

### Fix 5: Large File Handling (Deferred)

**Problem:** `read_jsonl_latest()` loads entire file into HashMap, which could OOM on very large files.

**Analysis:** True streaming with "latest wins" semantics is complex because:
- We must track the latest version of each record by ID
- This inherently requires O(unique IDs) memory
- The current implementation is already O(unique IDs), not O(file size)
- The JSON parsing per line is already streaming (BufReader)

**Current memory usage:** `HashMap<String, Value>` where:
- Keys: record IDs (typically UUIDs, ~36 bytes each)
- Values: JSON Value objects (size depends on record)

**Actual risk:** Only problematic if:
- Millions of unique records exist, OR
- Individual records are very large (megabytes each)

**Decision:** **DEFER** - The current implementation is acceptable:
- TaskDaemon is unlikely to have millions of records
- Individual records (Plans, Specs, Executions) are small
- No actual OOM has been observed

**Future mitigation (if needed):**
- Add file size check with warning/error above threshold (e.g., 100MB)
- Consider SQLite-only mode for high-volume collections
- Implement JSONL compaction (rewrite file with only latest versions)

## Alternatives Considered

### Alternative 1: Use SQLite as Source of Truth

**Description:** Store all data in SQLite, eliminate JSONL files entirely.

**Pros:**
- Native transactions and locking
- No sync complexity
- Better query performance

**Cons:**
- Loses git-trackable state
- Binary format not human-readable
- Harder to debug
- Merge conflicts become impossible to resolve

**Why not chosen:** Git-trackability is a core design requirement for TaskDaemon's multi-worktree coordination.

### Alternative 2: Use External Locking Service

**Description:** Use a lockfile or Redis for distributed locking instead of file-level locks.

**Pros:**
- Works across NFS/network filesystems
- More robust coordination

**Cons:**
- Additional dependency
- Overkill for single-machine use case
- Complexity

**Why not chosen:** TaskStore is designed for local git repositories. Advisory file locks are sufficient and simpler.

### Alternative 3: Store Index Schema in Metadata

**Description:** Store the indexed field names per collection in a metadata file, allowing sync to rebuild indexes.

**Pros:**
- Automatic index rebuilding during sync
- No need for `rebuild_indexes<T>()` call

**Cons:**
- Requires additional file/table
- Schema drift risk between metadata and actual Record implementations
- More complex sync logic

**Why not chosen:** The `rebuild_indexes<T>()` approach is simpler and ensures type safety.

## Technical Considerations

### Dependencies

**New:**
- `fs2 = "0.4"` - for file locking

**Existing (no changes):**
- rusqlite, serde, serde_json, chrono, etc.

### Performance

- File locking adds ~microseconds per write operation
- Index rebuilding is O(n) where n is number of records
- Staleness check is O(collections) - very fast
- No impact on read performance

### Security

- Advisory file locks are cooperative - malicious processes could ignore them
- ID validation prevents potential injection via empty IDs in SQL queries (defense in depth)
- No new attack surfaces introduced

### Backward Compatibility

All changes are backward compatible:

| Change | Impact |
|--------|--------|
| Cargo.toml edition | None - fixes build, no API change |
| File locking | None - internal implementation detail |
| sync_metadata table | Additive - new table, existing code unaffected |
| rebuild_indexes<T>() | Additive - new method, opt-in usage |
| ID validation | Minor - previously accepted empty IDs now rejected |

**Migration notes:**
- Existing `.taskstore` directories work without modification
- First `Store::open()` after upgrade creates `sync_metadata` table automatically
- If any code was using empty string IDs (unlikely), it will now fail at create time

### Testing Strategy

Each fix should have dedicated tests:

**1. Cargo.toml**: Build succeeds (`cargo check`)

**2. File locking** - concurrent write test:

```rust
#[test]
fn test_concurrent_writes_are_serialized() {
    let temp = TempDir::new().unwrap();
    let store_path = temp.path().to_path_buf();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let path = store_path.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                let record = TestRecord {
                    id: format!("concurrent-{}", i),
                    name: format!("Thread {}", i),
                    status: "active".to_string(),
                    count: i,
                    active: true,
                    updated_at: now_ms(),
                };
                store.create(record).unwrap();
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all records written correctly
    let store = Store::open(&store_path).unwrap();
    let records: Vec<TestRecord> = store.list(&[]).unwrap();
    assert_eq!(records.len(), 10);
}
```

**3. Index restoration** - filter after sync test:

```rust
#[test]
fn test_filters_work_after_sync_and_rebuild() {
    let temp = TempDir::new().unwrap();
    let mut store = Store::open(temp.path()).unwrap();

    // Create records with indexed status field
    store.create(TestRecord {
        id: "1".into(),
        name: "Record 1".into(),
        status: "active".into(),
        count: 1,
        active: true,
        updated_at: now_ms(),
    }).unwrap();
    store.create(TestRecord {
        id: "2".into(),
        name: "Record 2".into(),
        status: "draft".into(),
        count: 2,
        active: false,
        updated_at: now_ms(),
    }).unwrap();

    // Force sync (simulates git pull scenario - clears SQLite indexes)
    store.sync().unwrap();

    // Rebuild indexes for TestRecord type
    let count = store.rebuild_indexes::<TestRecord>().unwrap();
    assert_eq!(count, 2);

    // Filter should now work
    let active: Vec<TestRecord> = store.list(&[
        Filter { field: "status".into(), op: FilterOp::Eq, value: IndexValue::String("active".into()) }
    ]).unwrap();

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "1");
}
```

**4. Staleness detection**:

```rust
#[test]
fn test_modified_file_detected_as_stale() {
    let temp = TempDir::new().unwrap();
    let mut store = Store::open(temp.path()).unwrap();

    store.create(TestRecord {
        id: "1".into(),
        name: "Initial".into(),
        status: "active".into(),
        count: 1,
        active: true,
        updated_at: now_ms(),
    }).unwrap();
    drop(store);  // Close to release file handles

    // Simulate external modification (e.g., git merge adds a record)
    std::thread::sleep(std::time::Duration::from_millis(100));  // Ensure mtime changes
    let jsonl_path = temp.path().join(".taskstore/test_records.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&jsonl_path)
        .unwrap();
    use std::io::Write;
    writeln!(file, r#"{{"id":"2","name":"External","status":"draft","count":0,"active":false,"updated_at":9999}}"#).unwrap();
    drop(file);

    // Re-open store - should detect staleness and auto-sync
    let store = Store::open(temp.path()).unwrap();
    let record: Option<TestRecord> = store.get("2").unwrap();
    assert!(record.is_some(), "External record should be synced");
}

#[test]
fn test_new_collection_file_detected_as_stale() {
    let temp = TempDir::new().unwrap();
    let mut store = Store::open(temp.path()).unwrap();

    store.create(TestRecord {
        id: "1".into(),
        name: "Initial".into(),
        status: "active".into(),
        count: 1,
        active: true,
        updated_at: now_ms(),
    }).unwrap();
    drop(store);

    // Simulate new collection added externally (e.g., git merge from another branch)
    let new_jsonl = temp.path().join(".taskstore/other_collection.jsonl");
    std::fs::write(&new_jsonl, r#"{"id":"ext1","updated_at":1000}"#).unwrap();

    // Re-open - should detect the new file as stale
    let store = Store::open(temp.path()).unwrap();
    assert!(store.is_stale().unwrap());
}
```

**5. ID validation**:

```rust
#[test]
fn test_empty_id_rejected() {
    let temp = TempDir::new().unwrap();
    let mut store = Store::open(temp.path()).unwrap();

    let record = TestRecord {
        id: "".to_string(),  // Empty ID - should be rejected
        name: "Test".into(),
        status: "active".into(),
        count: 1,
        active: true,
        updated_at: now_ms(),
    };

    let result = store.create(record);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn test_whitespace_only_id_rejected() {
    let temp = TempDir::new().unwrap();
    let mut store = Store::open(temp.path()).unwrap();

    let record = TestRecord {
        id: "   ".to_string(),  // Whitespace-only ID - should be rejected
        name: "Test".into(),
        status: "active".into(),
        count: 1,
        active: true,
        updated_at: now_ms(),
    };

    let result = store.create(record);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}
```

### Rollout Plan

This is a library, not a service. Rollout is via version bump:

1. **Feature branch**: Create `phase0-fixes` branch
2. **Implementation**: Implement fixes in order (see Implementation Order)
3. **Unit tests**: Add tests as shown above
4. **Integration test**: Run TaskDaemon's test suite against new TaskStore
5. **Code review**: PR review focusing on locking correctness
6. **Merge**: Merge to main after approval
7. **Version bump**: 0.1.0 -> 0.2.0 (minor bump for new API)
8. **Update TaskDaemon**: Update Cargo.toml dependency, run tests
9. **Documentation**: Update TaskStore README with new `rebuild_indexes` API

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| File locking not supported on filesystem (NFS, FUSE) | Low | High | Document requirement; use local filesystems |
| Staleness check false negatives (mtime resolution) | Low | Low | Worst case: miss a sync; user can force `sync()` |
| Staleness check false positives | Low | Low | Extra sync is harmless, just unnecessary work |
| Wrong type passed to rebuild_indexes | Low | Low | Method skips non-matching records with warning |
| fs2 crate unmaintained | Low | Medium | Crate is stable; could vendor or replace with std (if stabilized) |
| ID validation breaks existing code | Very Low | Medium | Only affects empty/whitespace IDs, unlikely in practice |

## Open Questions

- [x] Should `rebuild_indexes` be called automatically after sync? **Decision: No, keep explicit for type safety**
- [x] Should streaming reader be included in Phase 0? **Decision: No, defer - current impl is already O(unique IDs)**
- [x] Should ID validation also check for invalid characters? **Decision: No, only check non-empty. IDs are often UUIDs or user-provided; restricting characters would limit flexibility without clear benefit.**
- [x] Should file locking use blocking or non-blocking mode? **Decision: Blocking (fs2 default). Non-blocking with retry would add complexity; blocking is simpler and sufficient for expected contention levels.**

## Implementation Order

1. Add fs2 dependency and file locking
2. Add sync_metadata table and improve is_stale()
3. Add rebuild_indexes<T>() method
4. Add ID validation

## References

- [Implementation Readiness Assessment](/home/saidler/repos/taskdaemon/taskdaemon/docs/implementation-readiness-assessment.md)
- [TaskStore Design Doc](/home/saidler/repos/taskdaemon/taskstore/docs/taskstore-design.md)
- [fs2 crate documentation](https://docs.rs/fs2)
- [Rust 2021 Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2021/)
