//! LRU decode cache for decoded images with configurable memory budget.
//!
//! ADR-008 §10E: T0 memory optimization — decode only viewport ± buffer;
//! discard on scroll. `ImageDecodeCache` is the central store for all decoded
//! images. Callers hold `ImageHandle` (an `Arc<Image>`) while rendering;
//! the cache evicts LRU entries when the memory budget is exceeded.

use std::collections::HashMap;
use std::sync::Arc;

use crate::Image;

/// A thin, reference-counted pointer to a decoded image stored in `ImageDecodeCache`.
///
/// As long as any `ImageHandle` is alive, the underlying pixel data stays in memory.
/// When the last handle is dropped and the cache evicts its entry, the data is freed.
pub type ImageHandle = Arc<Image>;

/// Cache key identifying a decoded image.
///
/// Typically the resource URL or a content hash string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageKey(pub String);

impl ImageKey {
    /// Construct from a URL or hash string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// Entry stored per cached image.
#[derive(Debug)]
struct CacheEntry {
    handle: ImageHandle,
    /// Decoded pixel data size in bytes (`width * height * bpp`).
    memory_bytes: usize,
    /// Logical timestamp of last access (insert or get). Used for LRU ordering.
    last_accessed: u64,
}

/// LRU decode cache for decoded raster images.
///
/// Default memory budget: 256 MB. When the budget is exceeded after an insert,
/// `evict_to_budget()` removes least-recently-used entries until usage falls
/// within budget.
///
/// Callers receive `ImageHandle` (`Arc<Image>`). The cache holds its own `Arc`
/// reference per entry; evicting an entry drops the cache's `Arc`. If a caller
/// still holds a handle, the pixel data remains alive until that handle is dropped.
#[derive(Debug)]
pub struct ImageDecodeCache {
    entries: HashMap<ImageKey, CacheEntry>,
    /// Current memory usage in bytes (sum of pixel data for all cached entries).
    used_bytes: usize,
    /// Memory budget in bytes. When `used_bytes > budget_bytes`, eviction runs.
    budget_bytes: usize,
    /// Monotonically increasing logical clock for LRU ordering.
    current_tick: u64,
}

/// Default memory budget: 256 MB.
const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;

impl ImageDecodeCache {
    /// Create a new cache with the default 256 MB budget.
    pub fn new() -> Self {
        Self::with_budget(DEFAULT_BUDGET)
    }

    /// Create a new cache with a custom memory budget in bytes.
    pub fn with_budget(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            used_bytes: 0,
            budget_bytes,
            current_tick: 0,
        }
    }

    /// Current memory used by all cached images (bytes).
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// Memory budget (bytes).
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// Number of cached images.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no images are cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` if the key is present in the cache.
    pub fn contains(&self, key: &ImageKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Look up a cached image by key, updating its LRU timestamp.
    ///
    /// Returns `None` if the key is not cached.
    pub fn get(&mut self, key: &ImageKey) -> Option<ImageHandle> {
        self.current_tick += 1;
        let tick = self.current_tick;
        self.entries.get_mut(key).map(|e| {
            e.last_accessed = tick;
            Arc::clone(&e.handle)
        })
    }

    /// Insert a decoded image into the cache and return a handle.
    ///
    /// If `key` is already cached, the existing entry's LRU timestamp is updated
    /// and its handle is returned (the new `image` is discarded).
    ///
    /// After insertion, `evict_to_budget()` runs automatically if usage exceeds
    /// the budget.
    pub fn insert(&mut self, key: ImageKey, image: Image) -> ImageHandle {
        self.current_tick += 1;
        let tick = self.current_tick;

        if let Some(e) = self.entries.get_mut(&key) {
            e.last_accessed = tick;
            return Arc::clone(&e.handle);
        }

        let memory_bytes = image.width as usize
            * image.height as usize
            * image.format.bytes_per_pixel();
        let handle = Arc::new(image);

        self.entries.insert(
            key,
            CacheEntry { handle: Arc::clone(&handle), memory_bytes, last_accessed: tick },
        );
        self.used_bytes = self.used_bytes.saturating_add(memory_bytes);

        if self.used_bytes > self.budget_bytes {
            self.evict_to_budget();
        }

        handle
    }

    /// Decode and cache an image, or return the existing cached handle.
    ///
    /// The closure is called only if `key` is not already in the cache.
    ///
    /// # Errors
    /// Propagates the closure's error string unchanged.
    pub fn decode_or_get<F>(&mut self, key: ImageKey, decode: F) -> Result<ImageHandle, String>
    where
        F: FnOnce() -> Result<Image, String>,
    {
        if let Some(h) = self.get(&key) {
            return Ok(h);
        }
        let image = decode()?;
        Ok(self.insert(key, image))
    }

    /// Evict least-recently-used entries until `used_bytes <= budget_bytes`.
    ///
    /// Entries are sorted by `last_accessed` ascending (oldest first); the oldest
    /// are removed until the budget is satisfied or the cache is empty.
    pub fn evict_to_budget(&mut self) {
        if self.used_bytes <= self.budget_bytes {
            return;
        }

        // Collect keys sorted by LRU (oldest first).
        let mut victims: Vec<(ImageKey, u64)> = self
            .entries
            .iter()
            .map(|(k, v)| (k.clone(), v.last_accessed))
            .collect();
        victims.sort_by_key(|(_, t)| *t);

        for (key, _) in victims {
            if self.used_bytes <= self.budget_bytes {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.used_bytes = self.used_bytes.saturating_sub(entry.memory_bytes);
            }
        }
    }

    /// Evict all cached entries regardless of budget.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }

    /// Return LRU candidates sorted from least- to most-recently used.
    ///
    /// Useful for callers that need to pre-evict before a large allocation.
    pub fn lru_candidates(&self) -> Vec<(ImageKey, u64)> {
        let mut v: Vec<_> =
            self.entries.iter().map(|(k, e)| (k.clone(), e.last_accessed)).collect();
        v.sort_by_key(|(_, t)| *t);
        v
    }

    /// React to an OS memory pressure event by evicting proportionally.
    ///
    /// - `Low`: no-op.
    /// - `Medium`: reduce effective budget to 50 % and run `evict_to_budget()`.
    /// - `High`: reduce effective budget to 10 % and run `evict_to_budget()`.
    ///
    /// The permanent `budget_bytes` field is unchanged; only the current
    /// eviction run uses the reduced target.
    pub fn on_memory_pressure(&mut self, level: lumen_core::MemoryPressureLevel) {
        use lumen_core::MemoryPressureLevel;
        let target = match level {
            MemoryPressureLevel::Low => return,
            MemoryPressureLevel::Medium => self.budget_bytes / 2,
            MemoryPressureLevel::High => self.budget_bytes / 10,
        };
        let saved = self.budget_bytes;
        self.budget_bytes = target;
        self.evict_to_budget();
        self.budget_bytes = saved;
    }
}

impl Default for ImageDecodeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl lumen_core::EvictableCache for ImageDecodeCache {
    fn on_memory_pressure(&mut self, level: lumen_core::MemoryPressureLevel) {
        ImageDecodeCache::on_memory_pressure(self, level);
    }

    fn used_bytes(&self) -> usize {
        // Inherent method returns usize directly.
        ImageDecodeCache::used_bytes(self)
    }

    fn budget_bytes(&self) -> usize {
        ImageDecodeCache::budget_bytes(self)
    }

    fn clear(&mut self) {
        ImageDecodeCache::clear(self);
    }

    fn cache_name(&self) -> &'static str {
        "image-decode-cache"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Image, PixelFormat};

    fn make_image(w: u32, h: u32) -> Image {
        let data = vec![255u8; w as usize * h as usize * 4];
        Image { width: w, height: h, format: PixelFormat::Rgba8, data, icc_profile: None }
    }

    fn key(s: &str) -> ImageKey {
        ImageKey::new(s)
    }

    fn bytes(w: u32, h: u32) -> usize {
        w as usize * h as usize * 4
    }

    #[test]
    fn insert_and_get_returns_same_data() {
        let mut cache = ImageDecodeCache::new();
        let img = make_image(100, 100);
        let h1 = cache.insert(key("a"), img.clone());
        let h2 = cache.get(&key("a")).unwrap();
        assert!(Arc::ptr_eq(&h1, &h2), "второй get должен вернуть тот же Arc");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn memory_accounting() {
        let mut cache = ImageDecodeCache::new();
        cache.insert(key("a"), make_image(100, 100));
        cache.insert(key("b"), make_image(200, 50));
        let expected = bytes(100, 100) + bytes(200, 50);
        assert_eq!(cache.used_bytes(), expected);
    }

    #[test]
    fn duplicate_insert_returns_existing_handle() {
        let mut cache = ImageDecodeCache::new();
        let h1 = cache.insert(key("a"), make_image(10, 10));
        let h2 = cache.insert(key("a"), make_image(10, 10));
        assert!(Arc::ptr_eq(&h1, &h2));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.used_bytes(), bytes(10, 10));
    }

    #[test]
    fn eviction_removes_lru_entry() {
        // Budget = 2 × 10×10 images.
        let budget = bytes(10, 10) * 2;
        let mut cache = ImageDecodeCache::with_budget(budget);

        cache.insert(key("a"), make_image(10, 10));
        cache.insert(key("b"), make_image(10, 10));
        // "a" accessed first → "a" is LRU at this point.
        // Touch "a" again to make "b" the LRU:
        cache.get(&key("a"));

        // Insert "c" → triggers eviction. "b" (LRU) should be evicted.
        cache.insert(key("c"), make_image(10, 10));

        assert!(cache.contains(&key("a")), "a должен остаться");
        assert!(!cache.contains(&key("b")), "b должен быть вытеснен");
        assert!(cache.contains(&key("c")), "c должен присутствовать");
        assert!(cache.used_bytes() <= budget);
    }

    #[test]
    fn decode_or_get_calls_closure_once() {
        let mut cache = ImageDecodeCache::new();
        let mut call_count = 0u32;

        cache
            .decode_or_get(key("a"), || {
                call_count += 1;
                Ok(make_image(10, 10))
            })
            .unwrap();
        cache
            .decode_or_get(key("a"), || {
                call_count += 1;
                Ok(make_image(10, 10))
            })
            .unwrap();

        assert_eq!(call_count, 1, "closure должен вызываться только при cache miss");
    }

    #[test]
    fn decode_or_get_propagates_error() {
        let mut cache = ImageDecodeCache::new();
        let result =
            cache.decode_or_get(key("bad"), || Err("decode failed".to_string()));
        assert!(result.is_err());
        assert!(!cache.contains(&key("bad")));
    }

    #[test]
    fn clear_resets_memory() {
        let mut cache = ImageDecodeCache::new();
        cache.insert(key("a"), make_image(100, 100));
        cache.insert(key("b"), make_image(200, 200));
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn lru_candidates_oldest_first() {
        let mut cache = ImageDecodeCache::new();
        cache.insert(key("a"), make_image(10, 10));
        cache.insert(key("b"), make_image(10, 10));
        cache.insert(key("c"), make_image(10, 10));
        // Access "a" last → it becomes MRU.
        cache.get(&key("a"));

        let cands = cache.lru_candidates();
        // Expect "b" or "c" first (both inserted before "a" access), "a" last.
        assert_eq!(cands.last().unwrap().0, key("a"), "a должен быть MRU");
    }

    #[test]
    fn held_handle_keeps_data_alive_after_eviction() {
        let budget = bytes(10, 10);
        let mut cache = ImageDecodeCache::with_budget(budget);

        let h = cache.insert(key("a"), make_image(10, 10));
        // Insert "b" → budget exceeded → "a" evicted from cache.
        cache.insert(key("b"), make_image(10, 10));

        assert!(!cache.contains(&key("a")), "a вытеснен из кэша");
        // But the handle "h" still holds the Arc — data is alive.
        assert_eq!(h.width, 10, "данные живы пока держим handle");
    }

    #[test]
    fn on_memory_pressure_low_noop() {
        let mut cache = ImageDecodeCache::new();
        cache.insert(key("a"), make_image(50, 50));
        cache.insert(key("b"), make_image(50, 50));
        let before = cache.used_bytes();
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::Low);
        assert_eq!(cache.used_bytes(), before, "Low не вытесняет");
    }

    #[test]
    fn on_memory_pressure_medium_evicts_half() {
        // Budget = 4 images. Fill 4 → used == budget. Medium should halve.
        let img_bytes = bytes(50, 50);
        let mut cache = ImageDecodeCache::with_budget(img_bytes * 4);
        for i in 0..4 {
            cache.insert(key(&i.to_string()), make_image(50, 50));
        }
        assert_eq!(cache.used_bytes(), img_bytes * 4);
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::Medium);
        assert!(cache.used_bytes() <= img_bytes * 2, "Medium должен оставить ≤50%");
    }

    #[test]
    fn on_memory_pressure_high_evicts_to_ten_percent() {
        let img_bytes = bytes(50, 50);
        let mut cache = ImageDecodeCache::with_budget(img_bytes * 4);
        for i in 0..4 {
            cache.insert(key(&i.to_string()), make_image(50, 50));
        }
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::High);
        assert!(
            cache.used_bytes() <= img_bytes * 4 / 10 + img_bytes,
            "High должен оставить ≤10% бюджета (или 1 изображение если бюджет/10 < одного изображения)"
        );
    }

    #[test]
    fn on_memory_pressure_restores_budget() {
        let img_bytes = bytes(10, 10);
        let budget = img_bytes * 10;
        let mut cache = ImageDecodeCache::with_budget(budget);
        cache.insert(key("a"), make_image(10, 10));
        cache.on_memory_pressure(lumen_core::MemoryPressureLevel::High);
        assert_eq!(cache.budget_bytes(), budget, "бюджет восстанавливается после эвикции");
    }
}
