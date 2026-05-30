//! Glyph atlas — кэш растеризованных глифов в одной 8-битной alpha-текстуре.
//!
//! Phase 0 — простейший shelf packer: атлас разбивается на горизонтальные
//! «полки», глиф кладётся слева направо в текущую полку; если ширина не
//! помещается — новая полка под текущей. Не оптимально по упаковке (есть
//! Skyline, MaxRects, Guillotine), но проще, и для 200–500 уникальных
//! глифов нашего scope хватает.
//!
//! Атлас сам по себе не знает про шрифты — принимает на вход уже
//! растеризованный `Bitmap` от `lumen-font::Rasterizer`. Так atlas
//! тестируется в изоляции, а renderer связывает font + atlas сам.
//!
//! Ключ кэша — структурированный `AtlasKey { face_id, glyph_id, size_bin,
//! coords_hash }`. Первые три поля закрывают **multi-size glyph caching**
//! (при `font-size: 16px` глифы растеризируются на bin 16 без
//! масштабирования, при 32px — на bin 32; раньше всё рисовалось на 24 px
//! с linear-sampler-blur). `coords_hash` — 64-битный хэш normalized
//! variation coords, который добавляет **variable-fonts caching**: один
//! `(face, glyph, size)` для двух разных `font-variation-settings` дают
//! разные записи и не перезаписывают друг друга. Empty coords (default
//! instance) → `coords_hash = 0`, ключ совпадает с pre-variable-fonts
//! поведением (backward-compatible).

use std::collections::HashMap;

use lumen_font::Bitmap;

/// Композитный ключ glyph-кэша. См. module-level docs.
///
/// Caller (Renderer) формирует key через `AtlasKey::new(...)`. `coords_hash`
/// вычисляется через `AtlasKey::hash_coords(coords)` из normalized axis
/// coordinates (Variable Fonts L1, normalized в `[-1.0, 1.0]` per axis,
/// длина = `Font::fvar().axis_count`). Empty coords → `hash_coords` → 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtlasKey {
    pub face_id: u16,
    pub glyph_id: u16,
    pub size_bin: u16,
    pub coords_hash: u64,
}

impl AtlasKey {
    pub fn new(face_id: u16, glyph_id: u16, size_bin: u16, coords_hash: u64) -> Self {
        Self { face_id, glyph_id, size_bin, coords_hash }
    }

    /// Стабильный 64-битный хэш normalized variation coords для cache key.
    /// Empty / all-zeros coords → 0 (совпадает с default-instance путём и
    /// pre-variable-fonts поведением). f32 хэшится через побитовое
    /// представление (`to_bits()`), что даёт детерминированную канонизацию
    /// `±0.0` и `NaN`-pattern-ов; для variation-coords это корректно (caller
    /// нормализует к финальной float-форме до хэширования).
    pub fn hash_coords(coords: &[f32]) -> u64 {
        if coords.is_empty() || coords.iter().all(|&c| c == 0.0) {
            return 0;
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for &c in coords {
            c.to_bits().hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphEntry {
    /// Левый-верхний угол глифа в атласе, в пикселях.
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub width: u32,
    pub height: u32,
    /// Timestamp последнего доступа (insert/access). Для LRU эвикции.
    pub last_accessed: u64,
}

#[derive(Debug)]
pub struct GlyphAtlas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    cache: HashMap<AtlasKey, GlyphEntry>,
    cursor_x: u32,
    shelf_y: u32,
    shelf_height: u32,
    /// Помечается при каждом `insert`. Renderer следит и заливает текстуру
    /// в GPU только когда `true`, потом вызывает `mark_clean()`.
    dirty: bool,
    /// Логический timestamp (инкрементируется при каждом доступе).
    /// Используется для LRU эвикции.
    current_tick: u64,
}

const PADDING: u32 = 1;

impl GlyphAtlas {
    pub fn new(size: u32) -> Self {
        assert!(size > 0);
        Self {
            width: size,
            height: size,
            pixels: vec![0u8; (size * size) as usize],
            cache: HashMap::new(),
            cursor_x: 0,
            shelf_y: 0,
            shelf_height: 0,
            dirty: true,
            current_tick: 0,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn get(&self, key: AtlasKey) -> Option<&GlyphEntry> {
        self.cache.get(&key)
    }

    /// Обновляет timestamp доступа для существующей записи.
    pub fn access(&mut self, key: AtlasKey) -> Option<GlyphEntry> {
        if let Some(entry) = self.cache.get_mut(&key) {
            self.current_tick = self.current_tick.saturating_add(1);
            entry.last_accessed = self.current_tick;
            return Some(*entry);
        }
        None
    }

    /// Возвращает список ключей отсортированных по last_accessed (от самого старого к новому).
    pub fn get_lru_candidates(&self) -> Vec<(AtlasKey, u64)> {
        let mut candidates: Vec<_> = self.cache
            .iter()
            .map(|(k, v)| (*k, v.last_accessed))
            .collect();
        candidates.sort_by_key(|(_, last_accessed)| *last_accessed);
        candidates
    }

    /// Удаляет записи с указанными ключами из кэша.
    pub fn remove_keys(&mut self, keys: &[AtlasKey]) -> usize {
        let mut removed = 0;
        for key in keys {
            if self.cache.remove(key).is_some() {
                removed += 1;
                self.dirty = true;
            }
        }
        removed
    }

    /// Кладёт растеризованный глиф в атлас. Возвращает `None` если место
    /// исчерпано. Если ключ уже в кэше — возвращает существующую запись
    /// без перезаписи пикселей (но обновляет last_accessed).
    pub fn insert(&mut self, key: AtlasKey, bitmap: &Bitmap) -> Option<GlyphEntry> {
        if self.cache.contains_key(&key) {
            self.current_tick = self.current_tick.saturating_add(1);
            if let Some(e) = self.cache.get_mut(&key) {
                e.last_accessed = self.current_tick;
                return Some(*e);
            }
            return None;
        }
        if bitmap.width == 0 || bitmap.height == 0 {
            return None;
        }
        if bitmap.width > self.width || bitmap.height > self.height {
            return None;
        }

        // Помещается ли в текущую полку?
        if self.cursor_x + bitmap.width + PADDING > self.width {
            // Открываем новую полку под текущей.
            self.shelf_y += self.shelf_height + PADDING;
            self.cursor_x = 0;
            self.shelf_height = 0;
        }

        // Влезает ли по вертикали?
        let glyph_bottom = self.shelf_y + bitmap.height.max(self.shelf_height);
        if glyph_bottom > self.height {
            return None;
        }

        let x = self.cursor_x;
        let y = self.shelf_y;
        for row in 0..bitmap.height {
            let src_off = (row * bitmap.width) as usize;
            let dst_off = ((y + row) * self.width + x) as usize;
            self.pixels[dst_off..dst_off + bitmap.width as usize]
                .copy_from_slice(&bitmap.pixels[src_off..src_off + bitmap.width as usize]);
        }

        self.cursor_x += bitmap.width + PADDING;
        self.shelf_height = self.shelf_height.max(bitmap.height);
        self.dirty = true;

        self.current_tick = self.current_tick.saturating_add(1);
        let entry = GlyphEntry {
            atlas_x: x,
            atlas_y: y,
            width: bitmap.width,
            height: bitmap.height,
            last_accessed: self.current_tick,
        };
        self.cache.insert(key, entry);
        Some(entry)
    }

    /// React to an OS memory pressure event by evicting glyphs from the cache.
    ///
    /// - `Low`: no-op.
    /// - `Medium`: remove the LRU 50 % of cached glyphs.
    /// - `High`: clear the entire glyph cache. Glyphs are re-rasterized on demand.
    ///
    /// After eviction the atlas texture pixels are NOT compacted — removed cache
    /// entries simply become unreachable and their atlas regions will be reused
    /// when the atlas is rebuilt (typically on the next full re-render).
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
                self.cache.clear();
                self.dirty = true;
                // Reset packing cursors so new glyphs fill from the top.
                self.cursor_x = 0;
                self.shelf_y = 0;
                self.shelf_height = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(w: u32, h: u32, fill: u8) -> Bitmap {
        Bitmap {
            width: w,
            height: h,
            pixels: vec![fill; (w * h) as usize],
            left: 0.0,
            top: 0.0,
        }
    }

    fn k(glyph: u16) -> AtlasKey {
        AtlasKey::new(0, glyph, 16, 0)
    }

    #[test]
    fn insert_single_glyph_at_origin() {
        let mut atlas = GlyphAtlas::new(64);
        let entry = atlas.insert(k(42), &bitmap(10, 12, 200)).unwrap();
        assert_eq!(entry.atlas_x, 0);
        assert_eq!(entry.atlas_y, 0);
        assert_eq!(entry.width, 10);
        assert_eq!(entry.height, 12);
        assert!(entry.last_accessed > 0);
        // Pixel at (0, 0) and (9, 11) — значение из исходного bitmap-а.
        assert_eq!(atlas.pixels()[0], 200);
        assert_eq!(atlas.pixels()[(11 * 64 + 9) as usize], 200);
        // Pixel за пределами глифа — нуль.
        assert_eq!(atlas.pixels()[11_usize], 0);
    }

    #[test]
    fn second_glyph_placed_after_first_with_padding() {
        let mut atlas = GlyphAtlas::new(64);
        atlas.insert(k(1), &bitmap(10, 12, 100)).unwrap();
        let e2 = atlas.insert(k(2), &bitmap(8, 10, 80)).unwrap();
        assert_eq!(e2.atlas_x, 11); // 10 + 1 padding
        assert_eq!(e2.atlas_y, 0);
    }

    #[test]
    fn cached_glyph_returns_existing_entry() {
        let mut atlas = GlyphAtlas::new(64);
        let first = atlas.insert(k(1), &bitmap(10, 10, 100)).unwrap();
        // Повторный insert с тем же ключом и даже другим bitmap — должен
        // вернуть первую запись, не перезаписывая место в атласе.
        let second = atlas.insert(k(1), &bitmap(20, 20, 200)).unwrap();
        assert_eq!(first.atlas_x, second.atlas_x);
        assert_eq!(first.atlas_y, second.atlas_y);
        // Размер остался от первого insert.
        assert_eq!(second.width, 10);
        // Но last_accessed обновилась
        assert!(second.last_accessed > first.last_accessed);
    }

    #[test]
    fn different_keys_store_separate_entries() {
        let mut atlas = GlyphAtlas::new(64);
        let key_16 = AtlasKey::new(0, 1, 16, 0);
        let key_32 = AtlasKey::new(0, 1, 32, 0);
        let e16 = atlas.insert(key_16, &bitmap(10, 12, 100)).unwrap();
        let e32 = atlas.insert(key_32, &bitmap(20, 24, 100)).unwrap();
        assert_ne!(
            (e16.atlas_x, e16.atlas_y),
            (e32.atlas_x, e32.atlas_y),
            "разные ключи занимают разные места"
        );
        assert_eq!(atlas.get(key_16).map(|e| e.atlas_x), Some(e16.atlas_x));
        assert_eq!(atlas.get(key_32).map(|e| e.atlas_x), Some(e32.atlas_x));
    }

    #[test]
    fn different_variation_coords_store_separate_entries() {
        // Variable-fonts invariant: тот же `(face, glyph, size)`, но разные
        // normalized variation coords → разные cache-записи. Без этого
        // wght=400 и wght=700 глиф 'A' перезаписывали бы друг друга в
        // атласе, и второй вариант рисовался бы из глюк-пикселей первого.
        let mut atlas = GlyphAtlas::new(64);
        let coords_a = [0.0_f32];
        let coords_b = [1.0_f32];
        let k_a = AtlasKey::new(0, 1, 16, AtlasKey::hash_coords(&coords_a));
        let k_b = AtlasKey::new(0, 1, 16, AtlasKey::hash_coords(&coords_b));
        assert_ne!(k_a, k_b, "разные coords дают разные ключи");
        let ea = atlas.insert(k_a, &bitmap(10, 12, 100)).unwrap();
        let eb = atlas.insert(k_b, &bitmap(10, 12, 200)).unwrap();
        assert_ne!((ea.atlas_x, ea.atlas_y), (eb.atlas_x, eb.atlas_y));
    }

    #[test]
    fn empty_coords_hash_equals_zero() {
        // Backward-compatible: до variable-fonts default-instance glyph
        // (coords == []) должен попадать в ту же запись что и pre-VF код,
        // где coords_hash отсутствовал. Hash от пустого slice = 0.
        assert_eq!(AtlasKey::hash_coords(&[]), 0);
        // All-zero coords (CSS default normalized) тоже даёт 0 — один
        // glyph не растеризируется дважды при безсодержательной cascade.
        assert_eq!(AtlasKey::hash_coords(&[0.0]), 0);
        assert_eq!(AtlasKey::hash_coords(&[0.0, 0.0, 0.0]), 0);
    }

    #[test]
    fn non_zero_coords_hash_is_non_zero() {
        // Любая ненулевая coord — нетривиальный hash. (Защита от случая
        // когда DefaultHasher для [1.0] случайно вернёт 0 — теоретически
        // возможно, но крайне маловероятно для конкретной таблицы.)
        assert_ne!(AtlasKey::hash_coords(&[1.0]), 0);
        assert_ne!(AtlasKey::hash_coords(&[0.5, -0.5]), 0);
    }

    #[test]
    fn coords_hash_distinguishes_axis_order() {
        // [0.5, 0.0] и [0.0, 0.5] — разные variation-instance в разных
        // осях (например, wght=high vs wdth=high). Хэш должен различать.
        let h1 = AtlasKey::hash_coords(&[0.5, 0.0]);
        let h2 = AtlasKey::hash_coords(&[0.0, 0.5]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn coords_hash_is_deterministic() {
        let h1 = AtlasKey::hash_coords(&[0.25, -0.5, 1.0]);
        let h2 = AtlasKey::hash_coords(&[0.25, -0.5, 1.0]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn new_shelf_when_row_overflows() {
        let mut atlas = GlyphAtlas::new(32);
        atlas.insert(k(1), &bitmap(20, 10, 100)).unwrap(); // (0, 0); cursor=21
        // 21 + 20 + 1 = 42 > 32 → новая полка.
        let e2 = atlas.insert(k(2), &bitmap(20, 10, 100)).unwrap();
        assert_eq!(e2.atlas_x, 0);
        assert_eq!(e2.atlas_y, 11); // 10 + 1 padding
    }

    #[test]
    fn returns_none_when_vertically_out_of_space() {
        let mut atlas = GlyphAtlas::new(24);
        // 4 глифа 10×10 поместятся: 2 на полке × 2 полки.
        for id in 1..=4 {
            assert!(atlas.insert(k(id), &bitmap(10, 10, 100)).is_some(), "id {id}");
        }
        // 5-й уже не помещается.
        assert!(atlas.insert(k(5), &bitmap(10, 10, 100)).is_none());
    }

    #[test]
    fn dirty_flag_lifecycle() {
        let mut atlas = GlyphAtlas::new(32);
        assert!(atlas.dirty()); // свежий атлас — dirty (нужна первая загрузка пустой текстуры).
        atlas.mark_clean();
        assert!(!atlas.dirty());
        atlas.insert(k(1), &bitmap(8, 8, 50)).unwrap();
        assert!(atlas.dirty());
        atlas.mark_clean();
        // Повторный insert уже существующего ключа — НЕ пометит dirty (ничего не записано).
        atlas.insert(k(1), &bitmap(8, 8, 50)).unwrap();
        assert!(!atlas.dirty());
    }

    #[test]
    fn oversized_glyph_rejected() {
        let mut atlas = GlyphAtlas::new(16);
        assert!(atlas.insert(k(1), &bitmap(20, 10, 100)).is_none());
        assert!(atlas.insert(k(2), &bitmap(10, 20, 100)).is_none());
    }

    #[test]
    fn zero_sized_bitmap_rejected() {
        let mut atlas = GlyphAtlas::new(32);
        assert!(atlas.insert(k(1), &bitmap(0, 10, 100)).is_none());
        assert!(atlas.insert(k(2), &bitmap(10, 0, 100)).is_none());
    }

    #[test]
    fn lru_candidates_sorted_by_access_time() {
        let mut atlas = GlyphAtlas::new(100);
        atlas.insert(k(1), &bitmap(10, 10, 100)).unwrap();
        atlas.insert(k(2), &bitmap(10, 10, 100)).unwrap();
        atlas.insert(k(3), &bitmap(10, 10, 100)).unwrap();

        let candidates = atlas.get_lru_candidates();
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].0, k(1));
        assert_eq!(candidates[1].0, k(2));
        assert_eq!(candidates[2].0, k(3));
        assert!(candidates[0].1 < candidates[1].1);
        assert!(candidates[1].1 < candidates[2].1);
    }

    #[test]
    fn access_updates_timestamp() {
        let mut atlas = GlyphAtlas::new(64);
        let k1 = k(1);
        let insert_entry = atlas.insert(k1, &bitmap(10, 10, 100)).unwrap();
        let access_entry = atlas.access(k1).unwrap();

        assert!(access_entry.last_accessed > insert_entry.last_accessed);
    }

    #[test]
    fn remove_keys_deletes_from_cache() {
        let mut atlas = GlyphAtlas::new(100);
        atlas.insert(k(1), &bitmap(10, 10, 100)).unwrap();
        atlas.insert(k(2), &bitmap(10, 10, 100)).unwrap();
        atlas.insert(k(3), &bitmap(10, 10, 100)).unwrap();

        let removed = atlas.remove_keys(&[k(1), k(3)]);
        assert_eq!(removed, 2);
        assert!(atlas.get(k(1)).is_none());
        assert!(atlas.get(k(2)).is_some());
        assert!(atlas.get(k(3)).is_none());
    }

    #[test]
    fn on_memory_pressure_low_noop() {
        let mut atlas = GlyphAtlas::new(128);
        for id in 1..=4 {
            atlas.insert(k(id), &bitmap(10, 10, 100)).unwrap();
        }
        let count_before = atlas.cache.len();
        atlas.on_memory_pressure(lumen_core::MemoryPressureLevel::Low);
        assert_eq!(atlas.cache.len(), count_before);
    }

    #[test]
    fn on_memory_pressure_medium_evicts_half() {
        let mut atlas = GlyphAtlas::new(256);
        for id in 1..=6 {
            atlas.insert(k(id), &bitmap(8, 8, 100)).unwrap();
        }
        atlas.on_memory_pressure(lumen_core::MemoryPressureLevel::Medium);
        assert!(atlas.cache.len() <= 3, "Medium должен оставить ≤50% глифов");
    }

    #[test]
    fn on_memory_pressure_high_clears_all() {
        let mut atlas = GlyphAtlas::new(256);
        for id in 1..=4 {
            atlas.insert(k(id), &bitmap(8, 8, 100)).unwrap();
        }
        atlas.on_memory_pressure(lumen_core::MemoryPressureLevel::High);
        assert_eq!(atlas.cache.len(), 0, "High должен очистить весь кэш глифов");
        assert!(atlas.dirty(), "High должен пометить атлас dirty");
        // После High новые глифы должны вставляться с начала.
        assert_eq!(atlas.cursor_x, 0);
        assert_eq!(atlas.shelf_y, 0);
    }
}
