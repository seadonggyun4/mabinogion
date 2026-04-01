use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{OpcUaError, OpcUaResult};

use super::{CompiledOpcUaSession, ImportedNodeSet, NodeSetSource, OpcUaSimulatorConfig};

#[derive(Debug, Clone, serde::Serialize)]
pub struct CompilationCacheReport {
    pub compilation_hit: bool,
    pub import_hits: usize,
    pub import_misses: usize,
    pub cache_dir: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ImportCacheCounters {
    pub(crate) hits: usize,
    pub(crate) misses: usize,
}

impl ImportCacheCounters {
    pub(crate) fn record_hit(&mut self) {
        self.hits += 1;
    }

    pub(crate) fn record_miss(&mut self) {
        self.misses += 1;
    }
}

pub(crate) struct ModelingCache {
    root: PathBuf,
}

impl ModelingCache {
    pub(crate) fn new() -> Self {
        Self {
            root: default_cache_root(),
        }
    }

    pub(crate) fn root_display(&self) -> String {
        self.root.display().to_string()
    }

    pub(crate) fn load_imported_nodeset(&self, key: &str) -> Option<ImportedNodeSet> {
        self.load_json("imports", key)
    }

    pub(crate) fn save_imported_nodeset(
        &self,
        key: &str,
        imported: &ImportedNodeSet,
    ) -> OpcUaResult<()> {
        self.save_json("imports", key, imported)
    }

    pub(crate) fn load_compiled_session(&self, key: &str) -> Option<CompiledOpcUaSession> {
        self.load_json("sessions", key)
    }

    pub(crate) fn save_compiled_session(
        &self,
        key: &str,
        compiled: &CompiledOpcUaSession,
    ) -> OpcUaResult<()> {
        self.save_json("sessions", key, compiled)
    }

    fn load_json<T: DeserializeOwned>(&self, kind: &str, key: &str) -> Option<T> {
        let path = self.root.join(kind).join(format!("{}.json", key));
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn save_json<T: Serialize>(&self, kind: &str, key: &str, value: &T) -> OpcUaResult<()> {
        let dir = self.root.join(kind);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", key));
        let temp_path = path.with_extension("json.tmp");
        let content = serde_json::to_vec_pretty(value)
            .map_err(|error| OpcUaError::Config(error.to_string()))?;
        fs::write(&temp_path, content)?;
        fs::rename(&temp_path, &path)?;
        Ok(())
    }
}

pub(crate) fn build_import_cache_key(
    source: &NodeSetSource,
    base_path: Option<&Path>,
) -> OpcUaResult<String> {
    let mut bytes = serde_json::to_vec(source).map_err(|error| {
        OpcUaError::Config(format!(
            "failed to serialize NodeSet source for cache key: {}",
            error
        ))
    })?;
    match source {
        NodeSetSource::File { path, .. } => {
            let resolved = resolve_path(base_path, path);
            let content = fs::read(&resolved).map_err(|error| {
                OpcUaError::Config(format!(
                    "failed to read NodeSet '{}' for cache key: {}",
                    resolved.display(),
                    error
                ))
            })?;
            bytes.extend_from_slice(&content);
        }
        NodeSetSource::Embedded { alias, .. } => {
            bytes.extend_from_slice(alias.as_bytes());
        }
    }
    Ok(stable_hash_bytes(&bytes))
}

pub(crate) fn build_compilation_cache_key(
    config: &OpcUaSimulatorConfig,
    session_name: &str,
    base_path: Option<&Path>,
) -> OpcUaResult<String> {
    let mut bytes = serde_json::to_vec(config).map_err(|error| {
        OpcUaError::Config(format!(
            "failed to serialize OPC UA simulator config for cache key: {}",
            error
        ))
    })?;
    bytes.extend_from_slice(session_name.as_bytes());
    for source in config.nodesets.values() {
        let import_key = build_import_cache_key(source, base_path)?;
        bytes.extend_from_slice(import_key.as_bytes());
    }
    Ok(stable_hash_bytes(&bytes))
}

fn default_cache_root() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if cfg!(target_os = "macos") {
            return home
                .join("Library")
                .join("Caches")
                .join("mabinogion")
                .join("mabi-opcua");
        }
        return home.join(".cache").join("mabinogion").join("mabi-opcua");
    }

    std::env::temp_dir().join("mabinogion").join("mabi-opcua")
}

fn resolve_path(base_path: Option<&Path>, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base_path) = base_path {
        base_path.parent().unwrap_or(base_path).join(path)
    } else {
        path.to_path_buf()
    }
}

fn stable_hash_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}
