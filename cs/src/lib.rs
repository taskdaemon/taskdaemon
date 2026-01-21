//! ContextStore - RLM-style external context store
//!
//! Provides unlimited context windows by storing large text corpora externally
//! and allowing LLMs to query them via tools (search, fetch chunks, window).
//!
//! # Architecture
//!
//! ```text
//! .contextstore/
//! └── {context_id}/
//!     ├── index.jsonl      # chunk metadata
//!     └── chunks/
//!         ├── 0001.txt
//!         ├── 0002.txt
//!         └── ...
//! ```
//!
//! # Example
//!
//! ```ignore
//! use contextstore::ContextStore;
//!
//! let store = ContextStore::open(".contextstore")?;
//! let ctx_id = store.ingest(&["docs/**/*.md"], 32 * 1024)?;
//! let matches = store.search(&ctx_id, "RLM.*recursive")?;
//! let chunk = store.get_chunk(&matches[0].chunk_id)?;
//! ```

pub mod cli;
pub mod config;
mod store;

pub use store::{ChunkMeta, ContextId, ContextStore, IngestOptions, SearchMatch, SearchOptions};

/// Default chunk size (32KB)
pub const DEFAULT_CHUNK_SIZE: usize = 32 * 1024;

/// Default overlap between chunks (2KB)
pub const DEFAULT_OVERLAP: usize = 2 * 1024;
