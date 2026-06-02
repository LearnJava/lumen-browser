//! GPU cache for `backdrop-filter` results (CSS Filter Effects L1 §2).
//!
//! The backdrop-filter blur is a multi-pass full-screen GPU operation that the
//! renderer re-runs on every frame, even when nothing behind the element has
//! changed. `BackdropCache` tracks, per backdrop element, a content hash of the
//! inputs that determine the filtered output. When a frame's backdrop inputs are
//! byte-identical to the previous frame, the cached filtered texture is reused
//! and the expensive blur passes are skipped.
//!
//! This module holds only the *metadata* (hashes + memory accounting); the GPU
//! textures live in the [`crate::renderer::Renderer`], keyed by the same
//! `ordinal`. Keeping the decision logic free of `wgpu` types makes it
//! unit-testable without a device.
//!
//! Invalidation is conservative: the renderer computes one hash over the whole
//! frame's display list (plus scroll offset and viewport size). Any change
//! anywhere invalidates every backdrop entry. This guarantees there are no
//! false cache hits (which would paint stale pixels); the cost is some false
//! misses when an unrelated part of the page changed. The win is repeated
//! identical frames (idle redraws, animation ticks that don't touch the page,
//! caret blink), where the blur passes are skipped entirely.

use std::collections::HashMap;

/// Default GPU memory budget for cached backdrop textures: 64 MB.
///
/// Each entry is a full parent-layer-sized RGBA8 texture (e.g. 1024×720×4 ≈
/// 2.8 MB), so 64 MB comfortably holds the handful of backdrop-filter elements
/// a realistic page uses.
pub const DEFAULT_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Per-element cache metadata. The matching GPU texture lives in the renderer,
/// keyed by the same `ordinal`.
struct Entry {
    /// Content hash of the inputs that produced the cached filtered texture.
    input_hash: u64,
    /// GPU memory footprint of the cached texture, in bytes.
    bytes: usize,
    /// LRU clock value at last access (lookup hit or store).
    last_used: u64,
}

/// Tracks freshness of cached `backdrop-filter` textures.
///
/// Keyed by `ordinal` — the position of the backdrop element among all
/// backdrop-filter elements in a frame, assigned in paint order. Identical
/// frames produce identical ordinals across redraws, so the same element maps
/// to the same cache slot.
pub struct BackdropCache {
    entries: HashMap<u32, Entry>,
    /// Monotonic LRU clock; bumped on every recorded access.
    clock: u64,
    /// Sum of `bytes` over all live entries.
    used_bytes: usize,
    /// Eviction threshold; entries are dropped (LRU first) once this is exceeded.
    budget_bytes: usize,
    /// When false, every lookup misses and nothing is stored (cache disabled).
    enabled: bool,
}

impl BackdropCache {
    /// Creates an enabled cache with [`DEFAULT_BUDGET_BYTES`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_budget(DEFAULT_BUDGET_BYTES)
    }

    /// Creates an enabled cache with a custom GPU memory budget (bytes).
    #[must_use]
    pub fn with_budget(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            used_bytes: 0,
            budget_bytes,
            enabled: true,
        }
    }

    /// Enables or disables the cache. Disabling clears all entries so the
    /// renderer drops the matching textures on the next frame.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.clear();
        }
    }

    /// Whether the cache is currently active.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns `true` (cache HIT) if an entry for `ordinal` exists with a
    /// matching `input_hash`. On a hit the entry's LRU position is refreshed.
    ///
    /// A disabled cache always misses. A miss does **not** mutate the stored
    /// hash — the renderer must call [`Self::store`] after producing fresh
    /// content.
    pub fn lookup(&mut self, ordinal: u32, input_hash: u64) -> bool {
        if !self.enabled {
            return false;
        }
        self.clock += 1;
        let clock = self.clock;
        match self.entries.get_mut(&ordinal) {
            Some(e) if e.input_hash == input_hash => {
                e.last_used = clock;
                true
            }
            _ => false,
        }
    }

    /// Records that `ordinal` now holds freshly produced content for
    /// `input_hash`, occupying `bytes` of GPU memory. Returns the ordinals of
    /// any entries evicted to stay within budget (the renderer must drop their
    /// textures). The just-stored `ordinal` is never evicted.
    ///
    /// No-op on a disabled cache (returns empty).
    pub fn store(&mut self, ordinal: u32, input_hash: u64, bytes: usize) -> Vec<u32> {
        if !self.enabled {
            return Vec::new();
        }
        self.clock += 1;
        let clock = self.clock;
        if let Some(prev) = self.entries.insert(
            ordinal,
            Entry { input_hash, bytes, last_used: clock },
        ) {
            self.used_bytes = self.used_bytes.saturating_sub(prev.bytes);
        }
        self.used_bytes += bytes;
        self.evict_to_budget(ordinal)
    }

    /// Drops the metadata entry for `ordinal`, if any. Returns `true` if an
    /// entry was removed. The renderer calls this when it recreates the backing
    /// texture (e.g. on viewport resize), so a stale hash never produces a hit
    /// against fresh GPU contents.
    pub fn invalidate(&mut self, ordinal: u32) -> bool {
        if let Some(e) = self.entries.remove(&ordinal) {
            self.used_bytes = self.used_bytes.saturating_sub(e.bytes);
            true
        } else {
            false
        }
    }

    /// Removes all entries. The renderer drops every backing texture in lockstep.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }

    /// Responds to a memory-pressure signal. Returns the ordinals whose textures
    /// the renderer must drop.
    ///
    /// - `Medium`: evict LRU entries until usage is at or below half the budget.
    /// - `High`: clear everything.
    /// - `Low`: no eviction.
    pub fn on_memory_pressure(&mut self, level: lumen_core::ext::MemoryPressureLevel) -> Vec<u32> {
        use lumen_core::ext::MemoryPressureLevel as L;
        match level {
            L::Low => Vec::new(),
            L::Medium => self.evict_to_target(self.budget_bytes / 2),
            L::High => {
                let dropped: Vec<u32> = self.entries.keys().copied().collect();
                self.clear();
                dropped
            }
        }
    }

    /// Number of live cache entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total GPU memory tracked by live entries, in bytes.
    #[must_use]
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// Configured eviction budget, in bytes.
    #[must_use]
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// Evicts LRU entries while usage exceeds the budget. `keep` is never
    /// evicted (it is the entry just stored). Returns evicted ordinals.
    fn evict_to_budget(&mut self, keep: u32) -> Vec<u32> {
        let mut evicted = Vec::new();
        while self.used_bytes > self.budget_bytes && self.entries.len() > 1 {
            let Some(victim) = self
                .entries
                .iter()
                .filter(|(k, _)| **k != keep)
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k)
            else {
                break;
            };
            if let Some(e) = self.entries.remove(&victim) {
                self.used_bytes = self.used_bytes.saturating_sub(e.bytes);
            }
            evicted.push(victim);
        }
        evicted
    }

    /// Evicts LRU entries until usage is at or below `target` bytes. Used by
    /// memory-pressure handling; unlike [`Self::evict_to_budget`] it may evict
    /// down to zero entries. Returns evicted ordinals.
    fn evict_to_target(&mut self, target: usize) -> Vec<u32> {
        let mut evicted = Vec::new();
        while self.used_bytes > target && !self.entries.is_empty() {
            let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k)
            else {
                break;
            };
            if let Some(e) = self.entries.remove(&victim) {
                self.used_bytes = self.used_bytes.saturating_sub(e.bytes);
            }
            evicted.push(victim);
        }
        evicted
    }
}

impl Default for BackdropCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::MemoryPressureLevel;

    #[test]
    fn miss_on_empty_cache() {
        let mut c = BackdropCache::new();
        assert!(!c.lookup(0, 0xABCD));
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn store_then_hit_on_same_hash() {
        let mut c = BackdropCache::new();
        c.store(0, 0xABCD, 1000);
        assert!(c.lookup(0, 0xABCD), "same hash must hit");
        assert_eq!(c.len(), 1);
        assert_eq!(c.used_bytes(), 1000);
    }

    #[test]
    fn miss_on_changed_hash() {
        let mut c = BackdropCache::new();
        c.store(0, 0xABCD, 1000);
        assert!(!c.lookup(0, 0x1234), "different hash must miss");
        // The stale entry is left in place; renderer overwrites it via store().
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn distinct_ordinals_are_independent() {
        let mut c = BackdropCache::new();
        c.store(0, 0xAAAA, 1000);
        c.store(1, 0xBBBB, 2000);
        assert!(c.lookup(0, 0xAAAA));
        assert!(c.lookup(1, 0xBBBB));
        assert!(!c.lookup(0, 0xBBBB));
        assert_eq!(c.used_bytes(), 3000);
    }

    #[test]
    fn store_overwrite_adjusts_used_bytes() {
        let mut c = BackdropCache::new();
        c.store(0, 0xAAAA, 1000);
        c.store(0, 0xBBBB, 4000);
        assert_eq!(c.len(), 1);
        assert_eq!(c.used_bytes(), 4000);
        assert!(c.lookup(0, 0xBBBB));
    }

    #[test]
    fn invalidate_removes_entry_and_bytes() {
        let mut c = BackdropCache::new();
        c.store(0, 0xAAAA, 1000);
        assert!(c.invalidate(0));
        assert!(!c.invalidate(0), "second invalidate is a no-op");
        assert_eq!(c.used_bytes(), 0);
        assert!(!c.lookup(0, 0xAAAA));
    }

    #[test]
    fn disabled_cache_always_misses_and_clears() {
        let mut c = BackdropCache::new();
        c.store(0, 0xAAAA, 1000);
        c.set_enabled(false);
        assert!(c.is_empty(), "disabling clears entries");
        assert!(!c.lookup(0, 0xAAAA));
        assert!(c.store(0, 0xAAAA, 1000).is_empty(), "store is a no-op while disabled");
        assert!(c.is_empty());
    }

    #[test]
    fn budget_eviction_drops_lru() {
        // Budget fits two 1000-byte entries but not three.
        let mut c = BackdropCache::with_budget(2500);
        assert!(c.store(0, 0xA, 1000).is_empty());
        // Touch ordinal 0 so it is more-recently-used than 1.
        let _ = c.store(1, 0xB, 1000);
        assert!(c.lookup(0, 0xA));
        // Storing the third entry exceeds budget → evict LRU (ordinal 1).
        let evicted = c.store(2, 0xC, 1000);
        assert_eq!(evicted, vec![1]);
        assert!(c.lookup(0, 0xA), "recently-used survivor");
        assert!(c.lookup(2, 0xC), "just-stored entry survives");
        assert!(!c.lookup(1, 0xB), "LRU evicted");
        assert!(c.used_bytes() <= c.budget_bytes());
    }

    #[test]
    fn store_never_evicts_itself() {
        // Even with a budget smaller than one entry, the just-stored entry stays.
        let mut c = BackdropCache::with_budget(500);
        let evicted = c.store(0, 0xA, 1000);
        assert!(evicted.is_empty());
        assert!(c.lookup(0, 0xA));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn memory_pressure_high_clears_all() {
        let mut c = BackdropCache::new();
        c.store(0, 0xA, 1000);
        c.store(1, 0xB, 1000);
        let mut dropped = c.on_memory_pressure(MemoryPressureLevel::High);
        dropped.sort_unstable();
        assert_eq!(dropped, vec![0, 1]);
        assert!(c.is_empty());
    }

    #[test]
    fn memory_pressure_medium_evicts_to_half_budget() {
        let mut c = BackdropCache::with_budget(4000); // half = 2000
        let _ = c.store(0, 0xA, 1000);
        let _ = c.store(1, 0xB, 1000);
        let _ = c.store(2, 0xC, 1000); // used = 3000 > 2000
        // Most-recently-stored is ordinal 2, then 1, then 0 (oldest).
        let evicted = c.on_memory_pressure(MemoryPressureLevel::Medium);
        assert_eq!(evicted, vec![0], "evict oldest until <= half budget");
        assert!(c.used_bytes() <= 2000);
        assert!(c.lookup(1, 0xB));
        assert!(c.lookup(2, 0xC));
    }

    #[test]
    fn memory_pressure_low_is_noop() {
        let mut c = BackdropCache::new();
        c.store(0, 0xA, 1000);
        assert!(c.on_memory_pressure(MemoryPressureLevel::Low).is_empty());
        assert!(c.lookup(0, 0xA));
    }
}
