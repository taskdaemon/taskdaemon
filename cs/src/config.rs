//! Configuration for contextstore

use eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the context store directory
    #[serde(default = "default_store_path")]
    pub store_path: PathBuf,

    /// Default chunk size in bytes
    #[serde(default = "default_chunk_size")]
    pub default_chunk_size: usize,

    /// Default overlap between chunks
    #[serde(default = "default_overlap")]
    pub default_overlap: usize,
}

fn default_store_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("contextstore")
}

fn default_chunk_size() -> usize {
    crate::DEFAULT_CHUNK_SIZE
}

fn default_overlap() -> usize {
    crate::DEFAULT_OVERLAP
}

impl Default for Config {
    fn default() -> Self {
        Self {
            store_path: default_store_path(),
            default_chunk_size: default_chunk_size(),
            default_overlap: default_overlap(),
        }
    }
}

impl Config {
    /// Load config from file, or use defaults
    pub fn load(path: Option<&PathBuf>) -> Result<Self> {
        if let Some(config_path) = path {
            let content = std::fs::read_to_string(config_path)?;
            let config: Config = serde_yaml::from_str(&content)?;
            return Ok(config);
        }

        // Try default locations
        let default_paths = [
            dirs::config_dir().map(|p| p.join("contextstore").join("config.yml")),
            Some(PathBuf::from("contextstore.yml")),
        ];

        for path in default_paths.iter().flatten() {
            if path.exists() {
                let content = std::fs::read_to_string(path)?;
                let config: Config = serde_yaml::from_str(&content)?;
                return Ok(config);
            }
        }

        Ok(Config::default())
    }

    /// Save config to file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
