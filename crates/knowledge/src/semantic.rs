//! §12.5 Semantic search over history — Step 3 of `docs/tasks/ph3-ai-module.md`.
//!
//! [`SemanticIndex`] is a linear-scan nearest-neighbour index over embedding
//! vectors. It stands in for a real HNSW index: the prerequisite doc
//! (`p2-knowledge-stemmer-hnsw.md`, referenced by the Step 3 brief) does not
//! exist in this repo yet, and Step 0 explicitly allows a mock/linear-scan
//! interface first. [`SemanticIndex::nearest`] is the only query surface, so
//! swapping in a real ANN index later is a drop-in replacement for the field
//! in [`crate::store::DefaultKnowledgeStore`] — no caller changes.
//!
//! Not persisted: rebuilt from scratch each process start, same as
//! [`crate::open_tabs::OpenTabsIndex`]. This crate does not depend on
//! `lumen-ai` — callers embed the query/text themselves (via
//! `lumen_ai::embedding::EmbeddingBackend`) and pass the resulting vector in.

use std::sync::Mutex;

/// One stored embedding plus the metadata needed to build a [`SemanticHit`]
/// without a second lookup into `HistoryFts`/`lumen-storage`.
struct SemanticEntry {
    rowid: i64,
    url: String,
    title: String,
    vector: Vec<f32>,
}

/// Result of a [`SemanticIndex::nearest`] query.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticHit {
    /// Row-id the embedding was indexed under; matches
    /// `lumen_storage::history::HistoryEntry.id`.
    pub rowid: i64,
    /// Page URL at indexing time.
    pub url: String,
    /// Page title at indexing time.
    pub title: String,
    /// Cosine similarity to the query vector, in `[-1.0, 1.0]`; higher = more similar.
    pub similarity: f32,
}

/// In-memory linear-scan nearest-neighbour index over embedding vectors.
/// Thread-safe via an internal `Mutex` (same pattern as
/// [`crate::open_tabs::OpenTabsIndex`]'s `Mutex<Connection>`).
pub struct SemanticIndex {
    entries: Mutex<Vec<SemanticEntry>>,
}

impl std::fmt::Debug for SemanticIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticIndex").finish()
    }
}

impl Default for SemanticIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self { entries: Mutex::new(Vec::new()) }
    }

    /// Insert or replace the embedding for `rowid`.
    pub fn insert(&self, rowid: i64, url: &str, title: &str, vector: Vec<f32>) {
        let mut entries = self.entries.lock().expect("SemanticIndex mutex poisoned");
        entries.retain(|e| e.rowid != rowid);
        entries.push(SemanticEntry { rowid, url: url.to_owned(), title: title.to_owned(), vector });
    }

    /// Remove the embedding for `rowid`, if present. No-op if absent.
    pub fn remove(&self, rowid: i64) {
        let mut entries = self.entries.lock().expect("SemanticIndex mutex poisoned");
        entries.retain(|e| e.rowid != rowid);
    }

    /// Return up to `limit` indexed entries with the highest cosine
    /// similarity to `query`, most similar first.
    ///
    /// O(n) linear scan over all stored vectors — fine for the small
    /// in-memory index Step 3 targets; replace [`SemanticIndex`] with a real
    /// ANN index before this needs to scale past a few thousand entries.
    pub fn nearest(&self, query: &[f32], limit: usize) -> Vec<SemanticHit> {
        let entries = self.entries.lock().expect("SemanticIndex mutex poisoned");
        let mut scored: Vec<SemanticHit> = entries
            .iter()
            .map(|e| SemanticHit {
                rowid: e.rowid,
                url: e.url.clone(),
                title: e.title.clone(),
                similarity: cosine_similarity(query, &e.vector),
            })
            .collect();
        scored.sort_by(|a, b| {
            b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        scored
    }
}

/// Cosine similarity between two vectors. Returns `0.0` for mismatched
/// lengths, empty vectors, or a zero-norm vector (undefined direction).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|y| y * y).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_returns_closest_synthetic_entry_first() {
        let index = SemanticIndex::new();
        // 10 synthetic entries scattered across a 3D embedding space.
        for i in 0..10i64 {
            let vector = vec![i as f32, (i * 2) as f32, (10 - i) as f32];
            index.insert(i, &format!("https://example.com/{i}"), &format!("Page {i}"), vector);
        }
        // Identical to entry 7's vector, so entry 7 must be the top hit.
        let query = vec![7.0, 14.0, 3.0];

        let hits = index.nearest(&query, 3);

        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].rowid, 7);
        assert_eq!(hits[0].url, "https://example.com/7");
        assert!((hits[0].similarity - 1.0).abs() < 1e-5);
    }

    #[test]
    fn nearest_limits_result_count() {
        let index = SemanticIndex::new();
        for i in 0..10i64 {
            index.insert(i, "https://example.com", "Page", vec![i as f32, 0.0]);
        }

        let hits = index.nearest(&[5.0, 0.0], 2);

        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn insert_replaces_existing_rowid() {
        let index = SemanticIndex::new();
        index.insert(1, "https://a.example", "A", vec![1.0, 0.0]);
        index.insert(1, "https://b.example", "B", vec![0.0, 1.0]);

        let hits = index.nearest(&[0.0, 1.0], 10);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://b.example");
    }

    #[test]
    fn nearest_on_empty_index_returns_empty() {
        let index = SemanticIndex::new();

        assert!(index.nearest(&[1.0, 0.0], 5).is_empty());
    }

    #[test]
    fn remove_drops_entry() {
        let index = SemanticIndex::new();
        index.insert(1, "https://a.example", "A", vec![1.0, 0.0]);
        index.remove(1);

        assert!(index.nearest(&[1.0, 0.0], 5).is_empty());
    }
}
