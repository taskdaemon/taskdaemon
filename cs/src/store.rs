//! Core ContextStore implementation

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use uuid::Uuid;

/// Unique identifier for a context
pub type ContextId = String;

/// Unique identifier for a chunk within a context
pub type ChunkId = String;

/// Metadata for a single chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    /// Unique chunk ID
    pub chunk_id: ChunkId,
    /// Source file path
    pub source: String,
    /// Byte offset in source file
    pub byte_start: u64,
    /// Byte end in source file
    pub byte_end: u64,
    /// Content hash for staleness detection
    pub content_hash: String,
    /// Creation timestamp (unix ms)
    pub created_at: i64,
}

/// Options for ingesting content
#[derive(Debug, Clone)]
pub struct IngestOptions {
    /// Size of each chunk in bytes
    pub chunk_size: usize,
    /// Overlap between adjacent chunks
    pub overlap: usize,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            chunk_size: crate::DEFAULT_CHUNK_SIZE,
            overlap: crate::DEFAULT_OVERLAP,
        }
    }
}

/// Options for searching
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Maximum number of results
    pub max_results: usize,
    /// Case insensitive search
    pub case_insensitive: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 10,
            case_insensitive: false,
        }
    }
}

/// A search match result
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// Chunk ID containing the match
    pub chunk_id: ChunkId,
    /// Byte offset within chunk
    pub offset: usize,
    /// Snippet of matching text
    pub snippet: String,
}

/// Statistics for a context
#[derive(Debug, Clone)]
pub struct ContextStats {
    /// Number of chunks
    pub chunk_count: usize,
    /// Total bytes stored
    pub total_bytes: u64,
    /// Number of source files
    pub source_count: usize,
}

/// The main context store
pub struct ContextStore {
    /// Base path for storage
    base_path: PathBuf,
}

impl ContextStore {
    /// Open or create a context store at the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let base_path = path.as_ref().to_path_buf();
        fs::create_dir_all(&base_path).context("Failed to create store directory")?;
        debug!(?base_path, "Opened context store");
        Ok(Self { base_path })
    }

    /// Ingest files matching the given patterns into a new context
    pub fn ingest(&self, patterns: &[String], options: IngestOptions) -> Result<ContextId> {
        let context_id = Uuid::now_v7().to_string();
        let ctx_path = self.base_path.join(&context_id);
        let chunks_path = ctx_path.join("chunks");
        fs::create_dir_all(&chunks_path)?;

        let index_path = ctx_path.join("index.jsonl");
        let mut index_file = fs::File::create(&index_path)?;

        let mut chunk_num = 0u32;

        for pattern in patterns {
            // Expand glob pattern
            let paths = glob::glob(pattern).context(format!("Invalid glob pattern: {}", pattern))?;

            for entry in paths {
                let path = entry?;
                if path.is_file() {
                    chunk_num = self.ingest_file(&path, &chunks_path, &mut index_file, chunk_num, &options)?;
                }
            }
        }

        info!(context_id, chunk_count = chunk_num, "Ingestion complete");
        Ok(context_id)
    }

    fn ingest_file(
        &self,
        path: &Path,
        chunks_path: &Path,
        index_file: &mut fs::File,
        mut chunk_num: u32,
        options: &IngestOptions,
    ) -> Result<u32> {
        let content = fs::read_to_string(path).context(format!("Failed to read file: {}", path.display()))?;
        let content_bytes = content.as_bytes();
        let source = path.to_string_lossy().to_string();

        let mut offset = 0usize;
        while offset < content_bytes.len() {
            let end = (offset + options.chunk_size).min(content_bytes.len());
            let chunk_content = &content_bytes[offset..end];

            chunk_num += 1;
            let chunk_id = format!("{:04}", chunk_num);
            let chunk_path = chunks_path.join(format!("{}.txt", chunk_id));

            fs::write(&chunk_path, chunk_content)?;

            let meta = ChunkMeta {
                chunk_id: chunk_id.clone(),
                source: source.clone(),
                byte_start: offset as u64,
                byte_end: end as u64,
                content_hash: format!("{:x}", md5_hash(chunk_content)),
                created_at: chrono::Utc::now().timestamp_millis(),
            };

            let line = serde_json::to_string(&meta)?;
            writeln!(index_file, "{}", line)?;

            // Move forward, accounting for overlap
            offset = if end >= content_bytes.len() { end } else { end - options.overlap };
        }

        Ok(chunk_num)
    }

    /// Search for a pattern within a context
    pub fn search(&self, context_id: &str, pattern: &str, options: SearchOptions) -> Result<Vec<SearchMatch>> {
        let ctx_path = self.base_path.join(context_id);
        let chunks_path = ctx_path.join("chunks");

        if !ctx_path.exists() {
            return Err(eyre::eyre!("Context not found: {}", context_id));
        }

        let regex = if options.case_insensitive {
            regex::RegexBuilder::new(pattern).case_insensitive(true).build()?
        } else {
            regex::Regex::new(pattern)?
        };

        let mut matches = Vec::new();

        for entry in fs::read_dir(&chunks_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "txt").unwrap_or(false) {
                let chunk_id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

                let content = fs::read_to_string(&path)?;

                for m in regex.find_iter(&content) {
                    let start = m.start().saturating_sub(30);
                    let end = (m.end() + 30).min(content.len());
                    let snippet = content[start..end].to_string();

                    matches.push(SearchMatch {
                        chunk_id: chunk_id.clone(),
                        offset: m.start(),
                        snippet,
                    });

                    if matches.len() >= options.max_results {
                        return Ok(matches);
                    }
                }
            }
        }

        Ok(matches)
    }

    /// Get the full content of a chunk
    pub fn get_chunk(&self, chunk_id: &str) -> Result<String> {
        // chunk_id format: "context_id/chunk_num" or just "chunk_num" if context known
        let (context_id, chunk_num) = if chunk_id.contains('/') {
            let parts: Vec<&str> = chunk_id.splitn(2, '/').collect();
            (parts[0], parts[1])
        } else {
            // Search all contexts for this chunk
            return Err(eyre::eyre!("Chunk ID must include context: context_id/chunk_num"));
        };

        let chunk_path = self
            .base_path
            .join(context_id)
            .join("chunks")
            .join(format!("{}.txt", chunk_num));

        fs::read_to_string(&chunk_path).context(format!("Chunk not found: {}", chunk_id))
    }

    /// Get a window of text around an offset
    pub fn get_window(&self, chunk_id: &str, center: usize, radius: usize) -> Result<String> {
        let content = self.get_chunk(chunk_id)?;
        let bytes = content.as_bytes();

        let start = center.saturating_sub(radius);
        let end = (center + radius).min(bytes.len());

        Ok(String::from_utf8_lossy(&bytes[start..end]).to_string())
    }

    /// Get statistics for a context
    pub fn stats(&self, context_id: &str) -> Result<ContextStats> {
        let ctx_path = self.base_path.join(context_id);
        let index_path = ctx_path.join("index.jsonl");

        if !ctx_path.exists() {
            return Err(eyre::eyre!("Context not found: {}", context_id));
        }

        let file = fs::File::open(&index_path)?;
        let reader = BufReader::new(file);

        let mut chunk_count = 0;
        let mut total_bytes = 0u64;
        let mut sources = std::collections::HashSet::new();

        for line in reader.lines() {
            let line = line?;
            let meta: ChunkMeta = serde_json::from_str(&line)?;
            chunk_count += 1;
            total_bytes += meta.byte_end - meta.byte_start;
            sources.insert(meta.source);
        }

        Ok(ContextStats {
            chunk_count,
            total_bytes,
            source_count: sources.len(),
        })
    }

    /// List all context IDs
    pub fn list_contexts(&self) -> Result<Vec<ContextId>> {
        let mut contexts = Vec::new();

        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                contexts.push(name.to_string());
            }
        }

        Ok(contexts)
    }

    /// Delete a context and all its data
    pub fn delete(&self, context_id: &str) -> Result<()> {
        let ctx_path = self.base_path.join(context_id);
        if ctx_path.exists() {
            fs::remove_dir_all(&ctx_path)?;
            info!(context_id, "Deleted context");
        }
        Ok(())
    }
}

/// Simple hash for content (not cryptographic, just for change detection)
fn md5_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_ingest_and_search() {
        let temp = TempDir::new().unwrap();
        let store_path = temp.path().join("store");
        let store = ContextStore::open(&store_path).unwrap();

        // Create a test file
        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "Hello world, this is a test of the RLM context store.").unwrap();

        let ctx_id = store
            .ingest(&[test_file.to_string_lossy().to_string()], IngestOptions::default())
            .unwrap();

        let matches = store.search(&ctx_id, "RLM", SearchOptions::default()).unwrap();
        assert!(!matches.is_empty());
        assert!(matches[0].snippet.contains("RLM"));
    }

    #[test]
    fn test_list_and_delete() {
        let temp = TempDir::new().unwrap();
        let store = ContextStore::open(temp.path()).unwrap();

        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "test content").unwrap();

        let ctx_id = store
            .ingest(&[test_file.to_string_lossy().to_string()], IngestOptions::default())
            .unwrap();

        let contexts = store.list_contexts().unwrap();
        assert!(contexts.contains(&ctx_id));

        store.delete(&ctx_id).unwrap();

        let contexts = store.list_contexts().unwrap();
        assert!(!contexts.contains(&ctx_id));
    }
}
