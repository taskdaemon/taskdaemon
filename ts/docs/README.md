# TaskStore Documentation

TaskStore is a generic Rust library providing durable, git-integrated persistent storage using the SQLite+JSONL+Git pattern (Bead Store pattern).

## Documentation Structure

### Core Design Documents

| Document | Description |
|----------|-------------|
| [storage-architecture.md](./storage-architecture.md) | Bead Store pattern: what goes where and why |
| [taskstore-design.md](./taskstore-design.md) | Full API design, schema, implementation plan |
| [implementation-guide.md](./implementation-guide.md) | Practical implementation details, patterns, examples |

### Read This First

If you're using TaskStore, read in this order:

1. **[storage-architecture.md](./storage-architecture.md)** - Understand the Bead Store pattern
2. **[taskstore-design.md](./taskstore-design.md)** - Full design (API, schema, alternatives)
3. **[implementation-guide.md](./implementation-guide.md)** - How to actually use it

### Quick Reference

**What is TaskStore?**
- Generic library + binary for persistent state management
- SQLite (fast queries) + JSONL (git-friendly) + Custom merge driver
- Works with any type implementing the `Record` trait

**Key Features:**
- Append-only JSONL (source of truth)
- Rebuildable SQLite cache
- Custom git merge driver for conflict resolution
- Git hooks for automatic sync
- Generic: no domain-specific types

**API Overview:**
```rust
use taskstore::{Store, Record, IndexValue, Filter, FilterOp};
use serde::{Serialize, Deserialize};

// Define your own types
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Plan {
    id: String,
    title: String,
    status: String,
    updated_at: i64,
}

impl Record for Plan {
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn collection_name() -> &'static str { "plans" }
    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        let mut fields = HashMap::new();
        fields.insert("status".to_string(), IndexValue::String(self.status.clone()));
        fields
    }
}

// Use the generic API
let mut store = Store::open(".")?;

// Create
let plan = Plan { /* ... */ };
store.create(plan)?;

// Query
let plans: Vec<Plan> = store.list(&[])?;
let plan: Option<Plan> = store.get("plan-001")?;

// Filter
let active: Vec<Plan> = store.list(&[
    Filter {
        field: "status".to_string(),
        op: FilterOp::Eq,
        value: IndexValue::String("active".to_string()),
    }
])?;

// Sync (rebuild SQLite from JSONL)
store.sync()?;
```

**CLI Usage:**
```bash
# Maintenance
taskstore sync

# Git integration
taskstore install-hooks
```

## Using TaskStore

TaskStore is consumed as a library. Define your own domain types and implement the `Record` trait:

```toml
[dependencies]
taskstore = { path = "../taskstore" }
serde = { version = "1.0", features = ["derive"] }
```

```rust
use taskstore::{Store, Record};

// Your domain types implement Record
impl Record for MyType {
    // ...
}

// Use the generic Store
let store = Store::open(".")?;
store.create(my_record)?;
```

## Status

TaskStore is implemented and production-ready. All core functionality is complete:
- ✅ Generic Record trait
- ✅ SQLite+JSONL dual storage
- ✅ Git merge driver
- ✅ Filtering system
- ✅ Collection-based storage
