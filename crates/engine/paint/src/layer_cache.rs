//! GPU layer cache with LRU eviction for off-viewport stacking contexts.
//!
//! Phase 2 ADR-008: T0 memory optimization through layer recycling.
//! Off-viewport stacking contexts (>3 screen heights from viewport) release textures.
//!
//! Struct `LayerCache` manages a pool of `wgpu::Texture` objects:
//! - Each texture can be reused for different layers (texture pool recycling)
//! - Textures are tracked by insertion order + last access time
//! - LRU eviction removes least-recently-used textures when GPU memory budget exceeded
//! - Coordinate with P3 (shell) for MemoryPressureSource event handling in future phases

use std::collections::HashMap;

/// Layer identification key for cache lookup.
/// Layers are identified by their stacking context and visual composition state.
/// Two different stacking contexts at same position/size get distinct cache entries.
///
/// For Phase 2, simplified to size-based + stacking context hash.
/// P4 may extend this with transform/filter/opacity fingerprinting later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerKey {
    /// Stacking context ID (unique per document layout cycle).
    pub stacking_context_id: u32,
    /// Texture dimensions (width, height) in physical pixels.
    pub width: u32,
    pub height: u32,
}

impl LayerKey {
    /// Create a new layer cache key.
    pub fn new(stacking_context_id: u32, width: u32, height: u32) -> Self {
        Self { stacking_context_id, width, height }
    }
}

/// Metadata for a cached GPU layer texture.
#[derive(Debug, Clone, Copy)]
pub struct LayerEntry {
    /// GPU memory used by this layer texture (width * height * 4 bytes for RGBA).
    pub memory_bytes: u32,
    /// Logical timestamp of last access (insert or access).
    /// Incremented per LayerCache operation.
    pub last_accessed: u64,
}

/// Layer cache managing GPU memory via LRU eviction.
///
/// `LayerCache` stores metadata about allocated layer textures (the actual `wgpu::Texture`
/// objects live in `Renderer`). This cache tracks access patterns and identifies candidates
/// for GPU memory reclamation.
///
/// Default GPU memory budget: 256 MB (configurable via `LayerCache::with_budget`).
#[derive(Debug)]
pub struct LayerCache {
    /// Cached layer metadata.
    cache: HashMap<LayerKey, LayerEntry>,
    /// GPU memory budget in bytes.
    budget_bytes: u32,
    /// Current total GPU memory in use.
    used_bytes: u32,
    /// Logical timestamp (incremented per operation for LRU).
    current_tick: u64,
    /// NodeId (u32) → LayerKey for nodes promoted via `will-change: transform/opacity/filter`.
    /// Populated by `promote_layer`; cleaned up by `demote_layer` / `sync_promoted_layers`.
    promoted_nodes: HashMap<u32, LayerKey>,
}

const DEFAULT_BUDGET: u32 = 256 * 1024 * 1024; // 256 MB

impl LayerCache {
    /// Create a new layer cache with default 256 MB GPU memory budget.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            budget_bytes: DEFAULT_BUDGET,
            used_bytes: 0,
            current_tick: 0,
            promoted_nodes: HashMap::new(),
        }
    }

    /// Create with custom GPU memory budget (in bytes).
    pub fn with_budget(budget_bytes: u32) -> Self {
        Self {
            cache: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
            current_tick: 0,
            promoted_nodes: HashMap::new(),
        }
    }

    /// Get the current GPU memory usage.
    pub fn used_bytes(&self) -> u32 {
        self.used_bytes
    }

    /// Get the GPU memory budget.
    pub fn budget_bytes(&self) -> u32 {
        self.budget_bytes
    }

    /// Check if adding a layer of given size would exceed budget.
    pub fn would_exceed_budget(&self, layer_memory: u32) -> bool {
        self.used_bytes.saturating_add(layer_memory) > self.budget_bytes
    }

    /// Insert or update a cached layer.
    /// Returns `true` if the layer was newly inserted, `false` if it was an existing update.
    /// Updates the access timestamp regardless.
    pub fn insert(&mut self, key: LayerKey, memory_bytes: u32) -> bool {
        self.current_tick += 1;

        match self.cache.get_mut(&key) {
            Some(entry) => {
                // Layer already cached — just update access time.
                entry.last_accessed = self.current_tick;
                false
            }
            None => {
                // New layer — add to cache and account for memory.
                self.cache.insert(
                    key,
                    LayerEntry { memory_bytes, last_accessed: self.current_tick },
                );
                self.used_bytes = self.used_bytes.saturating_add(memory_bytes);
                true
            }
        }
    }

    /// Mark a cached layer as accessed (used by current render).
    /// Updates last_accessed timestamp.
    pub fn access(&mut self, key: LayerKey) {
        self.current_tick += 1;
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.last_accessed = self.current_tick;
        }
    }

    /// Get candidates for LRU eviction, sorted from least- to most-recently-used.
    /// Caller should use this to select which textures to deallocate
    /// when GPU memory budget is exceeded.
    pub fn get_lru_candidates(&self) -> Vec<(LayerKey, u64)> {
        let mut candidates: Vec<_> =
            self.cache.iter().map(|(k, v)| (*k, v.last_accessed)).collect();
        candidates.sort_by_key(|(_, last_accessed)| *last_accessed);
        candidates
    }

    /// Remove cached layers by key, freeing GPU memory.
    /// Returns the number of layers successfully removed and total bytes freed.
    pub fn remove_keys(&mut self, keys: &[LayerKey]) -> (usize, u32) {
        let mut removed = 0;
        let mut freed_bytes: u32 = 0;

        for key in keys {
            if let Some(entry) = self.cache.remove(key) {
                removed += 1;
                freed_bytes = freed_bytes.saturating_add(entry.memory_bytes);
            }
        }

        self.used_bytes = self.used_bytes.saturating_sub(freed_bytes);
        (removed, freed_bytes)
    }

    /// Clear all cached entries (full eviction), including promoted layer registrations.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.used_bytes = 0;
        self.promoted_nodes.clear();
    }

    /// Get the number of cached layers.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Check if a specific layer is in cache.
    pub fn contains(&self, key: LayerKey) -> bool {
        self.cache.contains_key(&key)
    }

    /// Promote a node to its own GPU layer (for `will-change: transform/opacity/filter`).
    ///
    /// Inserts a `LayerCache` entry keyed by `node_id` (used as stacking context ID) at
    /// the given pixel size. Returns the `LayerKey` for the promoted layer.
    /// Idempotent: if already promoted with the same size, updates the access timestamp.
    /// // CSS: will-change — P4 wires ComputedStyle.will_change values to call this.
    pub fn promote_layer(&mut self, node_id: u32, width: u32, height: u32) -> LayerKey {
        let key = LayerKey::new(node_id, width.max(1), height.max(1));
        self.insert(key, width.max(1) * height.max(1) * 4);
        self.promoted_nodes.insert(node_id, key);
        key
    }

    /// Returns `true` if the given node has a promoted GPU layer.
    pub fn is_layer_promoted(&self, node_id: u32) -> bool {
        self.promoted_nodes.contains_key(&node_id)
    }

    /// Remove the promoted GPU layer for a node, freeing its cache entry.
    pub fn demote_layer(&mut self, node_id: u32) {
        if let Some(key) = self.promoted_nodes.remove(&node_id) {
            self.remove_keys(&[key]);
        }
    }

    /// Remove promoted layers for nodes NOT in `current_nodes`.
    ///
    /// Call after each relayout to clean up nodes removed from the DOM.
    pub fn sync_promoted_layers(&mut self, current_nodes: &[u32]) {
        let stale: Vec<u32> = self
            .promoted_nodes
            .keys()
            .filter(|id| !current_nodes.contains(id))
            .copied()
            .collect();
        for id in stale {
            self.demote_layer(id);
        }
    }

    /// Number of nodes currently promoted to their own GPU layer.
    pub fn promoted_count(&self) -> usize {
        self.promoted_nodes.len()
    }

    /// React to an OS memory pressure event by evicting GPU layer textures.
    ///
    /// - `Low`: no-op.
    /// - `Medium`: evict the LRU 50 % of layers to free GPU memory.
    /// - `High`: clear all cached layers (full GPU memory reclamation).
    pub fn on_memory_pressure(&mut self, level: lumen_core::MemoryPressureLevel) {
        use lumen_core::MemoryPressureLevel;
        match level {
            MemoryPressureLevel::Low => {}
            MemoryPressureLevel::Medium => {
                let mut candidates = self.get_lru_candidates();
                let evict_count = candidates.len() / 2;
                candidates.truncate(evict_count);
                let keys: Vec<_> = candidates.into_iter().map(|(k, _)| k).collect();
                self.remove_keys(&keys);
            }
            MemoryPressureLevel::High => {
                self.clear();
            }
        }
    }
}

impl Default for LayerCache {
    fn default() -> Self {
        Self::new()
    }
}

impl lumen_core::EvictableCache for LayerCache {
    fn on_memory_pressure(&mut self, level: lumen_core::MemoryPressureLevel) {
        LayerCache::on_memory_pressure(self, level);
    }

    fn used_bytes(&self) -> usize {
        // LayerCache::used_bytes() returns u32; cast to usize for the trait.
        LayerCache::used_bytes(self) as usize
    }

    fn budget_bytes(&self) -> usize {
        LayerCache::budget_bytes(self) as usize
    }

    fn clear(&mut self) {
        LayerCache::clear(self);
    }

    fn cache_name(&self) -> &'static str {
        "layer-cache"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: u32, w: u32, h: u32) -> LayerKey {
        LayerKey::new(id, w, h)
    }

    fn mem(w: u32, h: u32) -> u32 {
        w * h * 4 // RGBA
    }

    #[test]
    fn insert_and_access() {
        let mut cache = LayerCache::new();
        let k1 = key(1, 512, 512);

        assert!(cache.insert(k1, mem(512, 512)));
        assert!(!cache.insert(k1, mem(512, 512))); // Re-insert updates, not new
        assert_eq!(cache.used_bytes(), mem(512, 512));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn lru_ordering() {
        let mut cache = LayerCache::new();
        let k1 = key(1, 256, 256);
        let k2 = key(2, 256, 256);
        let k3 = key(3, 256, 256);

        cache.insert(k1, mem(256, 256));
        cache.insert(k2, mem(256, 256));
        cache.insert(k3, mem(256, 256));

        // Access k1 after k3 — k1 should become most recent
        cache.access(k1);

        let candidates = cache.get_lru_candidates();
        assert_eq!(candidates[0].0, k2); // k2 least recently used
        assert_eq!(candidates[1].0, k3); // k3 middle
        assert_eq!(candidates[2].0, k1); // k1 most recently used
    }

    #[test]
    fn budget_tracking() {
        let mut cache = LayerCache::with_budget(1_000_000);
        let k1 = key(1, 256, 256);

        cache.insert(k1, 256 * 256 * 4);
        assert!(!cache.would_exceed_budget(500_000)); // Under budget
        assert!(cache.would_exceed_budget(1_000_000)); // Over budget
    }

    #[test]
    fn remove_and_free() {
        let mut cache = LayerCache::new();
        let k1 = key(1, 256, 256);
        let k2 = key(2, 512, 512);
        let k3 = key(3, 512, 512);

        cache.insert(k1, mem(256, 256));
        cache.insert(k2, mem(512, 512));
        cache.insert(k3, mem(512, 512));

        let initial_used = cache.used_bytes();
        let (removed, freed) = cache.remove_keys(&[k1, k2]);

        assert_eq!(removed, 2);
        assert_eq!(freed, mem(256, 256) + mem(512, 512));
        assert_eq!(cache.used_bytes(), initial_used - freed);
        assert!(!cache.contains(k1));
        assert!(!cache.contains(k2));
        assert!(cache.contains(k3));
    }

    #[test]
    fn clear_all() {
        let mut cache = LayerCache::new();
        cache.insert(key(1, 512, 512), mem(512, 512));
        cache.insert(key(2, 512, 512), mem(512, 512));

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn eviction_workflow() {
        // Simulate eviction scenario: cache fills up, caller identifies LRU victims.
        let mut cache = LayerCache::with_budget(2_500_000); // 2.5 MB
        let k1 = key(1, 512, 512);
        let k2 = key(2, 512, 512);
        let k3 = key(3, 512, 512);
        let k4 = key(4, 512, 512);

        // Fill cache with 3 layers
        cache.insert(k1, mem(512, 512)); // 1 MB
        cache.insert(k2, mem(512, 512)); // 1 MB
        cache.insert(k3, mem(512, 512)); // 1 MB
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.used_bytes(), mem(512, 512) * 3);

        // We have 2.5 MB total, used 3 MB — would exceed on next insert
        assert!(cache.would_exceed_budget(mem(512, 512)));

        // Get LRU candidates (oldest first)
        let candidates = cache.get_lru_candidates();
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].0, k1); // k1 is oldest

        // Evict k1 (least recently used)
        let (removed, freed) = cache.remove_keys(&[k1]);
        assert_eq!(removed, 1);
        assert_eq!(freed, mem(512, 512));
        assert_eq!(cache.used_bytes(), mem(512, 512) * 2);

        // Now we have room for a new layer (2 MB used + 1 MB new = 3 MB, but 2.5 MB budget)
        // Actually, after removing k1, we have 2 MB used. Adding 1 MB would be 3 MB > 2.5 MB budget
        assert!(cache.would_exceed_budget(mem(512, 512)));

        // Evict k2 as well
        cache.remove_keys(&[k2]);
        assert_eq!(cache.used_bytes(), mem(512, 512)); // Only k3 remains

        // Now we have room
        assert!(!cache.would_exceed_budget(mem(512, 512)));
        cache.insert(k4, mem(512, 512));
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains(k1));
        assert!(!cache.contains(k2));
        assert!(cache.contains(k3));
        assert!(cache.contains(k4));
    }

    #[test]
    fn separate_layers_by_stacking_context() {
        // Different stacking contexts with same size should have separate entries
        let mut cache = LayerCache::new();
        let k1 = key(100, 256, 256);
        let k2 = key(101, 256, 256);

        cache.insert(k1, mem(256, 256));
        cache.insert(k2, mem(256, 256));

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.used_bytes(), mem(256, 256) + mem(256, 256));
        assert!(cache.contains(k1));
        assert!(cache.contains(k2));
    }

    #[test]
    fn on_memory_pressure_low_noop() {
        let mut cache = LayerCache::new();
        cache.insert(key(1, 256, 256), mem(256, 256));
        cache.insert(key(2, 256, 256), mem(256, 256));
        let before = cache.used_bytes();
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::Low);
        assert_eq!(cache.used_bytes(), before);
    }

    #[test]
    fn on_memory_pressure_medium_evicts_half() {
        let mut cache = LayerCache::new();
        for id in 1..=6 {
            cache.insert(key(id, 64, 64), mem(64, 64));
        }
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::Medium);
        assert!(cache.len() <= 3, "Medium должен оставить ≤50% слоёв");
    }

    #[test]
    fn on_memory_pressure_high_clears_all() {
        let mut cache = LayerCache::new();
        for id in 1..=4 {
            cache.insert(key(id, 128, 128), mem(128, 128));
        }
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::High);
        assert_eq!(cache.len(), 0, "High должен очистить все GPU-слои");
        assert_eq!(cache.used_bytes(), 0);
    }

    // ─── Promoted layers ──────────────────────────────────────────────────────

    #[test]
    fn promote_layer_inserts_cache_entry() {
        let mut cache = LayerCache::new();
        cache.promote_layer(42, 256, 256);
        assert!(cache.contains(LayerKey::new(42, 256, 256)));
        assert!(cache.used_bytes() > 0);
    }

    #[test]
    fn is_layer_promoted_true_after_promote() {
        let mut cache = LayerCache::new();
        assert!(!cache.is_layer_promoted(7));
        cache.promote_layer(7, 128, 128);
        assert!(cache.is_layer_promoted(7));
    }

    #[test]
    fn is_layer_promoted_false_for_unknown_node() {
        let cache = LayerCache::new();
        assert!(!cache.is_layer_promoted(99));
    }

    #[test]
    fn demote_layer_removes_from_cache() {
        let mut cache = LayerCache::new();
        cache.promote_layer(5, 64, 64);
        assert!(cache.is_layer_promoted(5));
        let before = cache.used_bytes();
        cache.demote_layer(5);
        assert!(!cache.is_layer_promoted(5));
        assert!(cache.used_bytes() < before, "demote должен освободить GPU-память");
    }

    #[test]
    fn promote_layer_deduplicates_node() {
        let mut cache = LayerCache::new();
        cache.promote_layer(10, 256, 256);
        cache.promote_layer(10, 256, 256); // повторно — не дублирует запись
        assert_eq!(cache.promoted_count(), 1);
    }

    #[test]
    fn sync_promoted_layers_removes_stale_nodes() {
        let mut cache = LayerCache::new();
        cache.promote_layer(1, 100, 100);
        cache.promote_layer(2, 100, 100);
        cache.promote_layer(3, 100, 100);
        // Только узлы 1 и 3 остались в дереве — узел 2 удалён.
        cache.sync_promoted_layers(&[1, 3]);
        assert!(cache.is_layer_promoted(1));
        assert!(!cache.is_layer_promoted(2), "стоявший узел должен быть demoted");
        assert!(cache.is_layer_promoted(3));
    }
}
