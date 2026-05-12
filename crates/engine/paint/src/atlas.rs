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

use std::collections::HashMap;

use lumen_font::Bitmap;

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
    cache: HashMap<u16, GlyphEntry>,
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

    pub fn get(&self, glyph_id: u16) -> Option<&GlyphEntry> {
        self.cache.get(&glyph_id)
    }

    /// Кладёт растеризованный глиф в атлас. Возвращает `None` если место
    /// исчерпано. Если глиф уже в кэше — возвращает существующую запись
    /// без перезаписи пикселей.
    pub fn insert(&mut self, glyph_id: u16, bitmap: &Bitmap) -> Option<GlyphEntry> {
        if let Some(&entry) = self.cache.get(&glyph_id) {
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
        self.cache.insert(glyph_id, entry);
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
        }
    }

    #[test]
    fn insert_single_glyph_at_origin() {
        let mut atlas = GlyphAtlas::new(64);
        let entry = atlas.insert(42, &bitmap(10, 12, 200)).unwrap();
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
        atlas.insert(1, &bitmap(10, 12, 100)).unwrap();
        let e2 = atlas.insert(2, &bitmap(8, 10, 80)).unwrap();
        assert_eq!(e2.atlas_x, 11); // 10 + 1 padding
        assert_eq!(e2.atlas_y, 0);
    }

    #[test]
    fn cached_glyph_returns_existing_entry() {
        let mut atlas = GlyphAtlas::new(64);
        let first = atlas.insert(1, &bitmap(10, 10, 100)).unwrap();
        // Повторный insert с тем же id и даже другим bitmap — должен вернуть
        // первую запись, не перезаписывая место в атласе.
        let second = atlas.insert(1, &bitmap(20, 20, 200)).unwrap();
        assert_eq!(first, second);
        // Размер остался от первого insert.
        assert_eq!(second.width, 10);
    }

    #[test]
    fn new_shelf_when_row_overflows() {
        let mut atlas = GlyphAtlas::new(32);
        atlas.insert(1, &bitmap(20, 10, 100)).unwrap(); // (0, 0); cursor=21
        // 21 + 20 + 1 = 42 > 32 → новая полка.
        let e2 = atlas.insert(2, &bitmap(20, 10, 100)).unwrap();
        assert_eq!(e2.atlas_x, 0);
        assert_eq!(e2.atlas_y, 11); // 10 + 1 padding
    }

    #[test]
    fn returns_none_when_vertically_out_of_space() {
        let mut atlas = GlyphAtlas::new(24);
        // 4 глифа 10×10 поместятся: 2 на полке × 2 полки.
        for id in 1..=4 {
            assert!(atlas.insert(id, &bitmap(10, 10, 100)).is_some(), "id {id}");
        }
        // 5-й уже не помещается.
        assert!(atlas.insert(5, &bitmap(10, 10, 100)).is_none());
    }

    #[test]
    fn dirty_flag_lifecycle() {
        let mut atlas = GlyphAtlas::new(32);
        assert!(atlas.dirty()); // свежий атлас — dirty (нужна первая загрузка пустой текстуры).
        atlas.mark_clean();
        assert!(!atlas.dirty());
        atlas.insert(1, &bitmap(8, 8, 50)).unwrap();
        assert!(atlas.dirty());
        atlas.mark_clean();
        // Повторный insert уже существующего — НЕ пометит dirty (ничего не записано).
        atlas.insert(1, &bitmap(8, 8, 50)).unwrap();
        assert!(!atlas.dirty());
    }

    #[test]
    fn oversized_glyph_rejected() {
        let mut atlas = GlyphAtlas::new(16);
        assert!(atlas.insert(1, &bitmap(20, 10, 100)).is_none());
        assert!(atlas.insert(2, &bitmap(10, 20, 100)).is_none());
    }

    #[test]
    fn zero_sized_bitmap_rejected() {
        let mut atlas = GlyphAtlas::new(32);
        assert!(atlas.insert(1, &bitmap(0, 10, 100)).is_none());
        assert!(atlas.insert(2, &bitmap(10, 0, 100)).is_none());
    }
}
