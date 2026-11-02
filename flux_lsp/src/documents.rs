//! Open document buffer + incremental edits + cached analysis.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use flux_diagnostics::SourceId;

use lsp_types::{Diagnostic, Position, TextDocumentContentChangeEvent, Url};

use crate::analysis::{analyze_document_with, position_to_offset, DocAnalysis};

#[derive(Debug, Clone)]
struct OpenDocument {
    text: String,
    version: i32,
}

#[derive(Debug, Clone)]
struct CachedDocAnalysis {
    content_hash: u64,
    /// Imported files the analysis read, with their content hash at the time
    /// (`None` if unreadable). A change to any of them makes the entry stale.
    deps: Vec<(PathBuf, Option<u64>)>,
    analysis: DocAnalysis,
}

/// In-memory document buffers with incremental sync and analysis reuse.
#[derive(Default)]
pub struct DocumentStore {
    docs: HashMap<Url, OpenDocument>,
    analysis: HashMap<Url, CachedDocAnalysis>,
    /// What each open document last reported on imported (not open) files.
    foreign: HashMap<Url, HashMap<Url, Vec<Diagnostic>>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, uri: Url, version: i32, text: String) {
        self.analysis.remove(&uri);
        self.docs.insert(uri, OpenDocument { text, version });
    }

    pub fn apply_changes(
        &mut self,
        uri: &Url,
        version: i32,
        changes: impl IntoIterator<Item = TextDocumentContentChangeEvent>,
    ) -> bool {
        let Some(doc) = self.docs.get_mut(uri) else {
            return false;
        };
        doc.version = version;
        for change in changes {
            apply_content_change(&mut doc.text, &change);
        }
        self.analysis.remove(uri);
        true
    }

    pub fn close(&mut self, uri: &Url) {
        self.docs.remove(uri);
        self.analysis.remove(uri);
    }

    pub fn is_open(&self, uri: &Url) -> bool {
        self.docs.contains_key(uri)
    }

    /// Records what `root` now reports on imported files; returns the files it
    /// reported on before.
    pub fn replace_foreign(
        &mut self,
        root: &Url,
        reports: HashMap<Url, Vec<Diagnostic>>,
    ) -> HashSet<Url> {
        let previous = if reports.is_empty() {
            self.foreign.remove(root)
        } else {
            self.foreign.insert(root.clone(), reports)
        };
        previous.unwrap_or_default().into_keys().collect()
    }

    /// What all open documents report on imported file `target`. A file has one
    /// diagnostics list per client, so one root must not overwrite another's.
    pub fn foreign_diagnostics(&self, target: &Url) -> Vec<Diagnostic> {
        let mut roots: Vec<&Url> = self.foreign.keys().collect();
        roots.sort_by_key(|root| root.to_string());
        let mut merged: Vec<Diagnostic> = Vec::new();
        for root in roots {
            for diagnostic in self.foreign[root].get(target).into_iter().flatten() {
                if !merged.contains(diagnostic) {
                    merged.push(diagnostic.clone());
                }
            }
        }
        merged
    }

    pub fn version(&self, uri: &Url) -> Option<i32> {
        self.docs.get(uri).map(|d| d.version)
    }

    pub fn text(&self, uri: &Url) -> Option<&str> {
        self.docs.get(uri).map(|d| d.text.as_str())
    }

    /// Returns cached analysis when neither the document text nor any imported
    /// file it read has changed since the last compile. Failed resolutions are
    /// never reused: a missing import may appear on disk at any time.
    pub fn analyze(&mut self, uri: &Url) -> Option<DocAnalysis> {
        let text = self.docs.get(uri)?.text.clone();
        let hash = content_hash(&text);
        // Imports read the editor's buffers, not the possibly older files.
        let overlay: HashMap<PathBuf, String> = self
            .docs
            .iter()
            .filter(|(open, _)| *open != uri)
            .filter_map(|(open, doc)| {
                Some((canonical(&open.to_file_path().ok()?), doc.text.clone()))
            })
            .collect();
        let hash_of = |path: &Path| match overlay.get(&canonical(path)) {
            Some(text) => Some(content_hash(text)),
            None => file_hash(path),
        };
        if let Some(cached) = self.analysis.get(uri) {
            let deps_fresh = cached
                .deps
                .iter()
                .all(|(path, dep_hash)| hash_of(path) == *dep_hash);
            if cached.content_hash == hash && deps_fresh && cached.analysis.hir.is_some() {
                return Some(cached.analysis.clone());
            }
        }
        let path = uri.to_file_path().ok();
        let analysis = analyze_document_with(path.as_deref(), &text, overlay.clone());
        let deps = analysis
            .source_paths
            .iter()
            .filter(|(id, _)| **id != SourceId(0))
            .map(|(_, path)| {
                let path = PathBuf::from(path);
                let hash = hash_of(&path);
                (path, hash)
            })
            .collect();
        self.analysis.insert(
            uri.clone(),
            CachedDocAnalysis {
                content_hash: hash,
                deps,
                analysis: analysis.clone(),
            },
        );
        Some(analysis)
    }

    /// Open documents (other than `uri`) whose last analysis read `uri`'s file
    /// or could not finish.
    pub fn dependents(&self, uri: &Url) -> Vec<Url> {
        let Some(path) = uri.to_file_path().ok().map(|p| canonical(&p)) else {
            return Vec::new();
        };
        let mut out: Vec<Url> = self
            .analysis
            .iter()
            .filter(|(root, cached)| {
                // A document that failed to resolve may have been missing `uri`.
                *root != uri
                    && self.docs.contains_key(*root)
                    && (cached.analysis.hir.is_none()
                        || cached.deps.iter().any(|(dep, _)| canonical(dep) == path))
            })
            .map(|(root, _)| root.clone())
            .collect();
        out.sort_by_key(|u| u.to_string());
        out
    }

    #[cfg(test)]
    pub fn analysis_cache_len(&self) -> usize {
        self.analysis.len()
    }
}

/// Apply a single LSP content change to `text` (incremental or full-buffer replace).
pub fn apply_content_change(text: &mut String, change: &TextDocumentContentChangeEvent) {
    match &change.range {
        None => *text = change.text.clone(),
        Some(range) => {
            let start = position_to_byte_offset(text, range.start);
            let end = position_to_byte_offset(text, range.end);
            if start <= end && end <= text.len() {
                text.replace_range(start..end, &change.text);
            }
        }
    }
}

fn position_to_byte_offset(text: &str, position: Position) -> usize {
    position_to_offset(text, position.line, position.character)
}

fn file_hash(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| content_hash(&text))
}

fn canonical(path: &Path) -> PathBuf {
    flux_driver::canonical_path(path)
}

fn content_hash(text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use lsp_types::Range;

    use super::*;

    #[test]
    fn apply_incremental_insert() {
        let mut text = "let x = 1\n".to_string();
        apply_content_change(
            &mut text,
            &TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 9), Position::new(0, 9))),
                range_length: None,
                text: "2".into(),
            },
        );
        assert_eq!(text, "let x = 12\n");
    }

    #[test]
    fn apply_incremental_replace() {
        let mut text = "let x = y + 1\n".to_string();
        apply_content_change(
            &mut text,
            &TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 8), Position::new(0, 9))),
                range_length: None,
                text: "5".into(),
            },
        );
        assert_eq!(text, "let x = 5 + 1\n");
    }

    #[test]
    fn apply_full_document_replace() {
        let mut text = "old\n".to_string();
        apply_content_change(
            &mut text,
            &TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "new\n".into(),
            },
        );
        assert_eq!(text, "new\n");
    }

    #[test]
    fn analysis_cache_hit_on_unchanged_document() {
        let uri = Url::parse("file:///tmp/cache_test.fl").unwrap();
        let mut store = DocumentStore::new();
        store.open(uri.clone(), 1, "let a = 1\n".into());
        let _ = store.analyze(&uri);
        assert_eq!(store.analysis_cache_len(), 1);
        let _ = store.analyze(&uri);
        assert_eq!(store.analysis_cache_len(), 1);
    }

    #[test]
    fn incremental_edit_invalidates_analysis_cache() {
        let uri = Url::parse("file:///tmp/cache_inv.fl").unwrap();
        let mut store = DocumentStore::new();
        store.open(uri.clone(), 1, "let a = 1\n".into());
        let _ = store.analyze(&uri);
        store.apply_changes(
            &uri,
            2,
            [TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 9), Position::new(0, 10))),
                range_length: None,
                text: "2".into(),
            }],
        );
        assert_eq!(store.analysis_cache_len(), 0);
    }
}
