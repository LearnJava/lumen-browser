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

    /// Кладёт растеризованный глиф в атлас. Возвращает `None` если место
    /// исчерпано. Если ключ уже в кэше — возвращает существующую запись
    /// без перезаписи пикселей.
    pub fn insert(&mut self, key: AtlasKey, bitmap: &Bitmap) -> Option<GlyphEntry> {
        if let Some(&entry) = self.cache.get(&key) {
            return Some(entry);
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

        let entry = GlyphEntry {
            atlas_x: x,
            atlas_y: y,
            width: bitmap.width,
            height: bitmap.height,
        };
        self.cache.insert(key, entry);
        Some(entry)
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
        assert_eq!(
            entry,
            GlyphEntry {
                atlas_x: 0,
                atlas_y: 0,
                width: 10,
                height: 12
            }
        );
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
        assert_eq!(first, second);
        // Размер остался от первого insert.
        assert_eq!(second.width, 10);
    }

    #[test]
    fn different_keys_store_separate_entries() {
        // Multi-size key invariant: (face_id, glyph_id, size_bin=16) и
        // (face_id, glyph_id, size_bin=32) для одного «глифа A» дают две
        // разные записи. Без этого мы бы перезаписывали единственную
        // запись (баг fixed-size атласа).
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
        assert_eq!(atlas.get(key_16).copied(), Some(e16));
        assert_eq!(atlas.get(key_32).copied(), Some(e32));
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
}
