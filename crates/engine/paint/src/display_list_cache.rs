//! Display list cache — stores pre-built `Vec<DisplayCommand>` per subtree root
//! (`NodeId`), keyed by a content hash. Lets the shell skip re-emitting display
//! list commands for stacking-context subtrees that did not change between frames.
//!
//! Phase 2 scope: full-page caching keyed by root `NodeId` + shell-level hash
//! check. Phase 3: per-stacking-context caching wired into `build_display_list_ordered`.
//!
//! Implements `EvictableCache` so `CacheRegistry` can broadcast memory-pressure
//! eviction signals across all caches (ADR-008 §10D.3).

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::display_list::DisplayCommand;

/// Cached display list for a stacking context or page subtree.
///
/// `content_hash` is derived from the commands at insertion time and used to
/// detect stale entries without re-traversing the layout tree.
#[derive(Debug, Clone)]
pub struct CachedDisplayLayer {
    /// Display-list commands for this subtree (paint order preserved).
    pub commands: Vec<DisplayCommand>,
    /// Content hash computed from `commands` at insertion time.
    /// A matching hash across frames means the display list is unchanged.
    pub content_hash: u64,
    /// Affine transform for compositor-offload of `will-change: transform` layers.
    /// Six-element column-major matrix `[a, b, c, d, tx, ty]`.
    /// `None` = normal document-flow layer (no compositor transform).
    pub transform: Option<[f32; 6]>,
    /// Byte footprint estimate: `commands.len() * size_of::<DisplayCommand>()`.
    pub byte_size: usize,
    /// Logical LRU timestamp — incremented per cache operation.
    pub(crate) last_accessed: u64,
}

/// LRU cache that maps `NodeId` (u32) to a pre-built `Vec<DisplayCommand>`.
///
/// Default memory budget: 32 MB. Eviction policy: LRU — least-recently-accessed
/// entries evicted first on budget overflow or `EvictableCache` pressure signal.
///
/// Register with `lumen_core::ext::CacheRegistry` so that OS memory-pressure
/// events trigger automatic eviction.
#[derive(Debug)]
pub struct DisplayListCache {
    entries: HashMap<u32, CachedDisplayLayer>,
    /// Current byte footprint across all cached display lists.
    used_bytes: usize,
    /// Budget in bytes (default 32 MB).
    budget_bytes: usize,
    /// Logical clock incremented per cache operation for LRU ordering.
    current_tick: u64,
}

const DEFAULT_BUDGET: usize = 32 * 1024 * 1024; // 32 MB

impl DisplayListCache {
    /// Create a cache with the default 32 MB budget.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            used_bytes: 0,
            budget_bytes: DEFAULT_BUDGET,
            current_tick: 0,
        }
    }

    /// Create with a custom byte budget.
    pub fn with_budget(budget_bytes: usize) -> Self {
        Self { entries: HashMap::new(), used_bytes: 0, budget_bytes, current_tick: 0 }
    }

    /// Look up the cached layer for `node_id`.
    ///
    /// Updates the LRU timestamp on hit. Returns `None` on miss.
    /// Callers should compare `entry.content_hash` with a freshly computed hash
    /// to confirm the entry is still valid.
    pub fn get(&mut self, node_id: u32) -> Option<&CachedDisplayLayer> {
        self.current_tick += 1;
        let tick = self.current_tick;
        if let Some(entry) = self.entries.get_mut(&node_id) {
            entry.last_accessed = tick;
            Some(entry)
        } else {
            None
        }
    }

    /// Insert or replace the cached display list for `node_id`.
    ///
    /// `content_hash` should be pre-computed via [`hash_display_list`].
    /// `transform` is `None` for normal-flow layers; `Some(affine)` for layers
    /// promoted via `will-change: transform`.
    ///
    /// Returns the byte footprint of the inserted entry.
    pub fn insert(
        &mut self,
        node_id: u32,
        commands: Vec<DisplayCommand>,
        content_hash: u64,
        transform: Option<[f32; 6]>,
    ) -> usize {
        let byte_size = commands.len() * std::mem::size_of::<DisplayCommand>();
        self.current_tick += 1;

        // Subtract old entry's memory before replacing.
        if let Some(old) = self.entries.remove(&node_id) {
            self.used_bytes = self.used_bytes.saturating_sub(old.byte_size);
        }

        self.entries.insert(
            node_id,
            CachedDisplayLayer {
                commands,
                content_hash,
                transform,
                byte_size,
                last_accessed: self.current_tick,
            },
        );
        self.used_bytes = self.used_bytes.saturating_add(byte_size);
        byte_size
    }

    /// Remove the cached layer for `node_id` and free its memory.
    pub fn remove(&mut self, node_id: u32) {
        if let Some(old) = self.entries.remove(&node_id) {
            self.used_bytes = self.used_bytes.saturating_sub(old.byte_size);
        }
    }

    /// Returns `true` if adding `extra_bytes` would exceed the budget.
    pub fn would_exceed_budget(&self, extra_bytes: usize) -> bool {
        self.used_bytes.saturating_add(extra_bytes) > self.budget_bytes
    }

    /// Evict LRU entries until at least `target_bytes` have been freed.
    ///
    /// Returns the total bytes freed.
    pub fn evict_lru(&mut self, target_bytes: usize) -> usize {
        if target_bytes == 0 {
            return 0;
        }
        // Collect (node_id, last_accessed) sorted oldest-first.
        let mut candidates: Vec<(u32, u64)> =
            self.entries.iter().map(|(id, e)| (*id, e.last_accessed)).collect();
        candidates.sort_by_key(|&(_, ts)| ts);

        let mut freed = 0usize;
        for (id, _) in candidates {
            if freed >= target_bytes {
                break;
            }
            if let Some(e) = self.entries.remove(&id) {
                freed = freed.saturating_add(e.byte_size);
                self.used_bytes = self.used_bytes.saturating_sub(e.byte_size);
            }
        }
        freed
    }

    /// Clear all cached entries and reset memory tracking.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current byte usage across all entries.
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// Configured budget in bytes.
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// React to an OS memory-pressure event.
    ///
    /// - `Low` → no-op.
    /// - `Medium` → evict the LRU 50 % of entries by byte count.
    /// - `High` → clear all entries.
    pub fn on_memory_pressure(&mut self, level: lumen_core::MemoryPressureLevel) {
        use lumen_core::MemoryPressureLevel;
        match level {
            MemoryPressureLevel::Low => {}
            MemoryPressureLevel::Medium => {
                let target = self.used_bytes / 2;
                self.evict_lru(target);
            }
            MemoryPressureLevel::High => {
                self.clear();
            }
        }
    }
}

impl Default for DisplayListCache {
    fn default() -> Self {
        Self::new()
    }
}

impl lumen_core::EvictableCache for DisplayListCache {
    fn on_memory_pressure(&mut self, level: lumen_core::MemoryPressureLevel) {
        DisplayListCache::on_memory_pressure(self, level);
    }

    fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    fn clear(&mut self) {
        DisplayListCache::clear(self);
    }

    fn cache_name(&self) -> &'static str {
        "display-list-cache"
    }
}

/// Compute a 64-bit content hash for a display-list command slice.
///
/// Uses `DefaultHasher` — not cryptographic, but fast and deterministic within
/// a single process run. Sufficient for per-subtree cache validity checks.
///
/// Note: the crate-level `hash_display_list` (in `display_list.rs`) takes
/// content + overlay + scroll + surface size and is used for full-frame dedup.
/// This function is for per-subtree (stacking-context) caching.
pub fn hash_commands(cmds: &[DisplayCommand]) -> u64 {
    let mut h = DefaultHasher::new();
    cmds.len().hash(&mut h);
    for cmd in cmds {
        // Hash the Debug representation: portable, deterministic within a run.
        format!("{cmd:?}").hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;
    use lumen_layout::Color;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    fn fill(x: f32, y: f32, w: f32, h: f32) -> DisplayCommand {
        DisplayCommand::FillRect {
            rect: rect(x, y, w, h),
            color: Color { r: 255, g: 0, b: 0, a: 255 },
        }
    }

    fn make_cmds() -> Vec<DisplayCommand> {
        vec![fill(0.0, 0.0, 100.0, 100.0), fill(10.0, 10.0, 50.0, 50.0)]
    }

    // ── Basic insert / get ────────────────────────────────────────────────

    #[test]
    fn insert_and_get_roundtrip() {
        let mut cache = DisplayListCache::new();
        let cmds = make_cmds();
        let hash = hash_commands(&cmds);
        cache.insert(1, cmds.clone(), hash, None);

        let entry = cache.get(1).expect("entry должна быть в кэше");
        assert_eq!(entry.commands, cmds);
        assert_eq!(entry.content_hash, hash);
        assert!(entry.transform.is_none());
    }

    #[test]
    fn get_miss_returns_none() {
        let mut cache = DisplayListCache::new();
        assert!(cache.get(99).is_none());
    }

    #[test]
    fn update_replaces_entry_no_duplicate() {
        let mut cache = DisplayListCache::new();
        let old = make_cmds();
        let h1 = hash_commands(&old);
        cache.insert(1, old, h1, None);

        let new_cmds = vec![fill(0.0, 0.0, 200.0, 200.0)];
        let h2 = hash_commands(&new_cmds);
        cache.insert(1, new_cmds.clone(), h2, None);

        assert_eq!(cache.len(), 1, "обновление не должно создавать дубликаты");
        let entry = cache.get(1).unwrap();
        assert_eq!(entry.commands, new_cmds);
        assert_eq!(entry.content_hash, h2);
    }

    // ── Memory budget ────────────────────────────────────────────────────

    #[test]
    fn memory_budget_tracking() {
        let mut cache = DisplayListCache::with_budget(100_000);
        let cmds = make_cmds();
        let size = cmds.len() * std::mem::size_of::<DisplayCommand>();
        let hash = hash_commands(&cmds);
        cache.insert(1, cmds, hash, None);

        assert_eq!(cache.used_bytes(), size);
        assert!(!cache.would_exceed_budget(100_000 - size));
        assert!(cache.would_exceed_budget(100_000));
    }

    // ── LRU eviction ────────────────────────────────────────────────────

    #[test]
    fn evict_lru_removes_oldest_first() {
        let mut cache = DisplayListCache::new();
        let cmds = make_cmds();
        let hash = hash_commands(&cmds);
        // Insert 3 entries in order: 1 is oldest, 3 is newest.
        cache.insert(1, cmds.clone(), hash, None);
        cache.insert(2, cmds.clone(), hash, None);
        cache.insert(3, cmds.clone(), hash, None);
        // Re-access node 1 to make it the most recent.
        let _ = cache.get(1);

        // Evict one entry's worth of bytes — should remove node 2 (oldest).
        let one = cmds.len() * std::mem::size_of::<DisplayCommand>();
        cache.evict_lru(one);

        assert!(cache.get(1).is_some(), "свежий узел (1) должен остаться");
        assert!(cache.get(3).is_some(), "второй свежий узел (3) должен остаться");
        assert_eq!(cache.len(), 2, "должна остаться ровно 2 записи");
    }

    // ── Memory pressure ──────────────────────────────────────────────────

    #[test]
    fn pressure_low_is_noop() {
        let mut cache = DisplayListCache::new();
        let cmds = make_cmds();
        let hash = hash_commands(&cmds);
        cache.insert(1, cmds, hash, None);
        let before = cache.used_bytes();
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::Low);
        assert_eq!(cache.used_bytes(), before, "Low не должен ничего удалять");
    }

    #[test]
    fn pressure_medium_evicts_half() {
        let mut cache = DisplayListCache::new();
        let cmds = make_cmds();
        let hash = hash_commands(&cmds);
        for id in 1u32..=6 {
            cache.insert(id, cmds.clone(), hash, None);
        }
        let before = cache.used_bytes();
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::Medium);
        // After evicting 50% target, used_bytes should be ≤ before/2 + one entry
        // (evict_lru stops as soon as target is met, possibly mid-entry).
        assert!(
            cache.used_bytes() <= before / 2 + cmds.len() * std::mem::size_of::<DisplayCommand>(),
            "Medium должен освободить ~50% памяти",
        );
    }

    #[test]
    fn pressure_high_clears_all() {
        let mut cache = DisplayListCache::new();
        let cmds = make_cmds();
        let hash = hash_commands(&cmds);
        for id in 1u32..=4 {
            cache.insert(id, cmds.clone(), hash, None);
        }
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::High);
        assert_eq!(cache.len(), 0, "High должен очистить кэш полностью");
        assert_eq!(cache.used_bytes(), 0);
    }

    // ── Hash utility ─────────────────────────────────────────────────────

    #[test]
    fn hash_differs_for_different_lists() {
        let a = vec![fill(0.0, 0.0, 100.0, 100.0)];
        let b = vec![fill(50.0, 50.0, 100.0, 100.0)];
        assert_ne!(
            hash_commands(&a),
            hash_commands(&b),
            "разные display list-ы должны давать разные хэши",
        );
    }

    // ── Transform field ──────────────────────────────────────────────────

    #[test]
    fn transform_stored_and_retrieved() {
        let mut cache = DisplayListCache::new();
        let cmds = make_cmds();
        let hash = hash_commands(&cmds);
        let tx = Some([1.0f32, 0.0, 0.0, 1.0, 50.0, 100.0]); // translate(50, 100)
        cache.insert(1, cmds, hash, tx);
        let entry = cache.get(1).unwrap();
        assert_eq!(entry.transform, tx);
    }
}
