//! File-level compilation cache keyed by source hash (including import dependencies).
//!
//! Cache files live under `<project_root>/.flux/cache/` — not in a global home directory.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use flux_hir::HirProgram;
use flux_ir::CompiledProgram;
use flux_resolve::find_project_root;
use flux_resolve::ResolvedProgram;
use flux_types::TypedProgram;
use sha2::{Digest, Sha256};

use crate::paths::{cache_dir, ensure_project_dirs};

const CACHE_MAGIC: &[u8; 6] = b"FLXIR\0";
const CACHE_VERSION: u8 = 1;

#[derive(Default)]
pub struct CompileCache {
    project_root: PathBuf,
    entries: HashMap<String, Vec<u8>>,
}

impl CompileCache {
    /// Cache scoped to the current working directory's project root.
    pub fn new() -> Self {
        Self::for_project(default_project_root())
    }

    /// In-memory cache scoped to the project containing `entry`.
    pub fn for_entry(entry: &Path) -> Self {
        Self::for_project(find_project_root(entry))
    }

    /// In-memory cache scoped to `project_root` (directory with or without `flux.toml`).
    pub fn for_project(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            entries: HashMap::new(),
        }
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn cache_dir(&self) -> PathBuf {
        cache_dir(&self.project_root)
    }

    pub fn hash_source(source: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Cache key from root source plus transitive dependency file contents.
    pub fn cache_key_resolved(resolved: &ResolvedProgram) -> String {
        let mut hasher = Sha256::new();
        let mut modules: Vec<_> = resolved.modules.iter().collect();
        modules.sort_by_key(|m| m.source.path.as_str());
        for module in modules {
            hasher.update(module.source.path.as_bytes());
            hasher.update(module.source.contents.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    fn cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir().join(format!("{key}.ir"))
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(hit) = self.entries.get(key) {
            return Some(hit.clone());
        }
        fs::read(self.cache_path(key)).ok()
    }

    /// Deserialize a cached [`CompiledProgram`], if the blob is valid.
    pub fn get_program(&self, key: &str) -> Option<CompiledProgram> {
        self.get(key).and_then(|bytes| deserialize_program(&bytes))
    }

    pub fn put(&mut self, key: &str, data: Vec<u8>) {
        self.entries.insert(key.to_string(), data.clone());
        if ensure_project_dirs(&self.project_root).is_ok() {
            let _ = fs::write(self.cache_path(key), data);
        }
    }

    pub fn put_program(&mut self, key: &str, program: &CompiledProgram) {
        self.put(key, serialize_program(program));
    }

    pub fn invalidate(&mut self, key: &str) {
        self.entries.remove(key);
        let _ = fs::remove_file(self.cache_path(key));
    }
}

/// Serialize a full IR program for on-disk cache storage.
pub fn serialize_program(program: &CompiledProgram) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(CACHE_MAGIC);
    out.push(CACHE_VERSION);
    out.extend(bincode::serialize(program).expect("CompiledProgram must be serializable"));
    out
}

/// Deserialize a cached IR program. Returns `None` for missing, corrupt, or legacy blobs.
pub fn deserialize_program(bytes: &[u8]) -> Option<CompiledProgram> {
    if bytes.len() < CACHE_MAGIC.len() + 1 {
        return None;
    }
    if &bytes[..CACHE_MAGIC.len()] != CACHE_MAGIC {
        return None;
    }
    if bytes[CACHE_MAGIC.len()] != CACHE_VERSION {
        return None;
    }
    bincode::deserialize(&bytes[CACHE_MAGIC.len() + 1..]).ok()
}

pub fn cache_key_for_path(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    Some(CompileCache::hash_source(&contents))
}

/// Returns true when a cached blob round-trips to the same program bytes.
pub fn cache_matches(cached: &[u8], program: &CompiledProgram) -> bool {
    deserialize_program(cached)
        .is_some_and(|decoded| serialize_program(&decoded) == serialize_program(program))
}

fn default_project_root() -> PathBuf {
    std::env::current_dir()
        .ok()
        .map(|cwd| find_project_root(&cwd))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Cached type-check artifacts keyed by [`CompileCache::cache_key_resolved`].
#[derive(Clone)]
pub struct CachedAnalysis {
    pub hir: HirProgram,
    pub typed: TypedProgram,
    pub types: HashMap<String, String>,
}

static ANALYSIS_CACHE: OnceLock<Mutex<HashMap<String, CachedAnalysis>>> = OnceLock::new();

pub fn get_analysis_cache(key: &str) -> Option<CachedAnalysis> {
    ANALYSIS_CACHE.get()?.lock().ok()?.get(key).cloned()
}

pub fn put_analysis_cache(key: &str, entry: CachedAnalysis) {
    if let Ok(mut cache) = ANALYSIS_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.insert(key.to_string(), entry);
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub fn clear_analysis_cache() {
    if let Ok(mut cache) = ANALYSIS_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_diagnostics::{DiagnosticBag, SourceId};
    use flux_hir::lower_program;
    use flux_ir::lower_to_ir;
    use flux_syntax::parse_program;

    #[test]
    fn cache_dir_is_project_local() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/cache-dir-test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("flux.toml"), "[package]\nname = \"t\"\n").unwrap();
        let cache = CompileCache::for_entry(&root.join("main.fl"));
        assert_eq!(cache.cache_dir(), root.join(".flux").join("cache"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn program_roundtrip_preserves_nodes() {
        let src = "let a = 1\nlet b = 2\n";
        let mut diag = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diag).unwrap();
        let hir = lower_program(&ast, &mut diag).unwrap();
        let ir = lower_to_ir(&hir, SourceId(0), &mut diag).unwrap();
        let blob = serialize_program(&ir);
        let restored = deserialize_program(&blob).expect("deserialize");
        assert_eq!(restored.nodes.len(), ir.nodes.len());
        assert!(cache_matches(&blob, &ir));
    }

    #[test]
    fn legacy_metadata_blob_is_cache_miss() {
        let legacy = b"legacy-metadata-not-a-program";
        assert!(deserialize_program(legacy).is_none());
    }
}
